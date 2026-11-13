#!/usr/bin/env python3
"""Spec A9 (#820), one slice per run. `--slice f1` (the default): CI nodes, the CI contract, `trace`
through an intermediate pipeline, external files, orphans, absent files, `ref mismatch` and the files the
CI reader owns. `--slice f2`: the command table of a `run` script, programs of the workspace, the builder
sites and the chain of three contracts. `--slice f3`: declared permissions and `permissions mismatch`.

Answer keys: #820 (the table under "Further Notes", the forms of `uses`, ADR 0028) and this
driver's own reading of the files it generates, never Meada's answer. Each case is marked in its
generated file by `# case: <id>` and `# control: <id>`: the control is a case with another
expected answer, so an answer given without reading the case fails one of the two.
`--selftest` runs without Meada. A script pass is evidence, not a PROVED verdict.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time

import yaml

import prove
from prove import Http, answer, fetch, free_port, git, mcp_answers

HOST = 'github.com/acme'
W = '.github/workflows/'
# The optional fields of a consumer row in `meada export`, as #824 named them.
REF, WITH, SECRETS = 'ref', 'with', 'secrets'
ABSENT = 'absent at the indexed commit'  # #820's words for the mark
# The consumer row's field for the declared permissions, as #834 named it.
PERMISSIONS = 'permissions'
NOT_DECLARED, PMISMATCH = 'not declared', 'permissions mismatch'  # #820's words
LEVELS = ('none', 'read', 'write')  # #820: the levels, in their order
REASON = re.compile(r'@(\S+) \(indexed ([0-9a-f]{7,64})\)(.*)')  # #820, "Git ref"; the flags follow
ORPHAN = re.compile(r'\s*orphan (ci::\S+): ')  # the orphan line of `meada status`, as every kind prints it
EXTERNAL = ('actions/checkout', 'actions/setup-node', 'docker/build-push-action', 'other-org/')
CORPUS = {'ci-slsa': 'github.com/slsa-framework/slsa-github-generator', 'ci-osv-scanner': 'github.com/google/osv-scanner',
          'ci-osv-action': 'github.com/google/osv-scanner-action'}
SOLO = '<solo>'  # the identity Meada gives a repository without `origin`, read from its export


def flagged(text):
    """A CI link's reason as (Git ref, indexed commit, flags), or None in any other shape. #820 fixes the
    words of each flag and not their order: the flags are `, ref mismatch` alone or hold `permissions mismatch`."""
    shape = REASON.fullmatch(text)
    return (shape[1], shape[2], shape[3]) if shape and (PMISMATCH in shape[3] or shape[3] in ('', ', ref mismatch')) else None


def pipeline(repo, file):
    """The contract and the provider site of a reusable pipeline, as #820 writes them."""
    return f'ci::{HOST}/{repo}/{W}{file}', f'{HOST}/{repo}::{W}{file}::.'


def action(repo, folder='', file='action.yml'):
    path = f'{folder}/{file}' if folder else file
    return f'ci::{HOST}/{repo}' + (f'/{folder}' if folder else ''), f'{HOST}/{repo}::{path}::.'


def link(target, ref=None):
    return ('link', *target, ref)


def mismatch(target, ref):
    return ('mismatch', *target, ref)


def absent(target):
    return ('absent', target[0])


UNRESOLVED, NONE = ('unresolved',), ('none',)
PUBLISH, RELEASE = pipeline('workflows', 'publish.yml'), pipeline('wrapper', 'release.yml')
LOCAL, LOCAL_ACTION = pipeline('forms', 'local.yml'), action('forms', 'local-action')
BUILD = pipeline('ref-provider', 'build.yml')
HEAD_REF, OTHER_40, OTHER_64, SHORT = 'PROVIDER_HEAD_SHA', '0123456789abcdef' * 2 + '01234567', '0123456789abcdef' * 4, '0123456'

# id: (workspace, repository, file, job or `runs` or None, step id or None, expected answer).
# The first four are #820's table under "Further Notes" and the fifth its `trace` sentence; the rest
# are one row per form of `uses`
# and per "certain or absent" probe of its "Testing Decisions".
CASES = {
    'a_publish': ('calls', 'app-a', W + 'ci.yml', 'publish', None, link(PUBLISH, 'main')),
    'b_publish_prd': ('calls', 'app-b', W + 'ci.yml', 'publish-prd', None, link(PUBLISH, 'v1')),
    'b_publish_stg': ('calls', 'app-b', W + 'ci.yml', 'publish-stg', None, link(PUBLISH, 'main')),
    'w_release': ('calls', 'wrapper', W + 'release.yml', 'release', None, link(PUBLISH, 'main')),
    'c_release': ('calls', 'app-c', W + 'ci.yml', 'release', None, link(RELEASE, 'main')),
    'a_checkout': ('calls', 'app-a', W + 'ci.yml', 'build', 'a_checkout', NONE),
    'a_image': ('calls', 'app-a', W + 'ci.yml', 'build', 'a_image', NONE),
    'b_checkout': ('calls', 'app-b', W + 'ci.yml', 'build', 'b_checkout', NONE),
    'b_image': ('calls', 'app-b', W + 'ci.yml', 'build', 'b_image', NONE),
    'job_remote': ('calls', 'forms', W + 'forms.yml', 'job_remote', None, link(pipeline('workflows', 'on-string.yml'), 'v1')),
    'job_external': ('calls', 'forms', W + 'forms.yml', 'job_external', None, NONE),
    'job_local': ('calls', 'forms', W + 'forms.yml', 'job_local', None, link(LOCAL)),
    'job_local_missing': ('calls', 'forms', W + 'forms.yml', 'job_local_missing', None, absent(pipeline('forms', 'missing.yml'))),
    'job_dollar': ('calls', 'forms', W + 'forms.yml', 'job_dollar', None, link(LOCAL)),
    'job_owner_case': ('calls', 'forms', W + 'forms.yml', 'job_owner_case', None, link(pipeline('workflows', 'on-list.yml'), 'main')),
    'job_path_case': ('calls', 'forms', W + 'forms.yml', 'job_path_case', None, absent(pipeline('workflows', 'On-List.yml'))),
    'job_on_map': ('calls', 'forms', W + 'forms.yml', 'job_on_map', None, link(pipeline('workflows', 'on-map.yml'), 'main')),
    'job_not_reusable': ('calls', 'forms', W + 'forms.yml', 'job_not_reusable', None, absent(pipeline('workflows', 'plain.yml'))),
    'step_remote_root': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_remote_root', link(action('actions'), 'v1')),
    'step_remote_folder': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_remote_folder', link(action('actions', 'setup'), 'v1')),
    'step_only_yaml': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_only_yaml',
                       link(action('actions', 'only-yaml', 'action.yaml'), 'v1')),
    'step_docker_only': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_docker_only', NONE),
    'step_missing_folder': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_missing_folder', absent(action('actions', 'nowhere'))),
    'step_docker_ref': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_docker_ref', NONE),
    'step_external': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_external', NONE),
    'step_local_own': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_local_own', link(LOCAL_ACTION)),
    'step_dollar': ('calls', 'forms', W + 'forms.yml', 'steps_own_checkout', 'step_dollar', link(LOCAL_ACTION)),
    'step_local_no_checkout': ('calls', 'forms', W + 'forms.yml', 'steps_no_checkout', 'step_local_no_checkout', UNRESOLVED),
    'step_local_other_repository': ('calls', 'forms', W + 'forms.yml', 'steps_checkout_repository', 'step_local_other_repository', UNRESOLVED),
    'step_local_checkout_path': ('calls', 'forms', W + 'forms.yml', 'steps_checkout_path', 'step_local_checkout_path', UNRESOLVED),
    'step_local_in_reusable': ('calls', 'forms', W + 'local.yml', 'inner', 'step_local_in_reusable', UNRESOLVED),
    'step_dollar_in_reusable': ('calls', 'forms', W + 'local.yml', 'inner', 'step_dollar_in_reusable', link(LOCAL_ACTION)),
    'step_local_in_composite': ('calls', 'forms', 'local-action/action.yml', 'runs', 'step_local_in_composite', UNRESOLVED),
    'step_dollar_in_composite': ('calls', 'forms', 'local-action/action.yml', 'runs', 'step_dollar_in_composite',
                                 link(action('forms', 'local-action/nested'))),
    # The files the CI reader owns (#828): the base gives the first three a use by the generic rule.
    'use_typed_key': ('calls', 'forms', W + 'uses.yml', 'deploy', 'use_typed_key', ('no use',)),
    'use_structured': ('calls', 'forms', W + 'uses.yml', 'deploy', 'use_structured', ('use on the CI step',)),
    'use_role_arn': ('calls', 'forms', W + 'uses.yml', 'deploy', 'use_role_arn', ('no use',)),
    'use_other_file': ('calls', 'forms', '.github/settings.yml', None, None, ('use as the base',)),
    'ref_equal': ('refs', 'ref-consumer', W + 'ci.yml', 'ref_equal', None, link(BUILD, HEAD_REF)),
    'ref_differs': ('refs', 'ref-consumer', W + 'ci.yml', 'ref_differs', None, mismatch(BUILD, OTHER_40)),
    'ref_differs_64': ('refs', 'ref-consumer', W + 'ci.yml', 'ref_differs_64', None, mismatch(BUILD, OTHER_64)),
    'ref_short': ('refs', 'ref-consumer', W + 'ci.yml', 'ref_short', None, link(BUILD, SHORT)),
    'orphan_unused': ('orphan', 'lonely', W + 'unused.yml', None, None, ('orphan', pipeline('lonely', 'unused.yml')[0])),
    'orphan_used': ('orphan', 'lonely-caller', W + 'ci.yml', 'call', None, link(pipeline('lonely', 'used.yml'), 'main')),
    'orphan_no_scope': ('orphan-control', 'unreferenced', W + 'unused.yml', None, None, ('no orphan',)),
    'solo_local': ('solo', 'solo', W + 'ci.yml', 'solo_local', None,
                   ('link', f'ci::{SOLO}/{W}build.yml', f'{SOLO}::{W}build.yml::.', None)),
    'solo_remote': ('solo', 'solo', W + 'ci.yml', 'solo_remote', None, NONE),
    # Nobody links to the peer's pipeline, and the scope seen from `solo` holds a provider and a consumer.
    'solo_peer_unused': ('solo', 'solo-peer', W + 'build.yml', None, None, ('orphan', pipeline('solo-peer', 'build.yml')[0])),
}

REUSABLE = '''name: NAME
on: ON
jobs:
  run:
    runs-on: ubuntu-latest
    steps:
      - run: echo NAME
'''
COMPOSITE = '''name: NAME
runs:
  using: composite
  steps:
    - run: echo NAME
      shell: bash
'''


def reusable(name, on='workflow_call'):
    return REUSABLE.replace('NAME', name).replace('ON', on)


# workspace -> repository -> file -> text. A hub and each repository's peers are added by `seed`.
WORKSPACES = {
    'calls': {
        'workflows': {
            W + 'publish.yml': '''name: publish
on:
  workflow_call:
    inputs:
      package:
        type: string
        required: true
permissions:
  id-token: write   # OIDC publish: every caller job must grant this
  contents: read
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: echo "publish ${{ inputs.package }}"
''',
            W + 'on-string.yml': reusable('on-string'),
            W + 'on-list.yml': reusable('on-list', '[push, workflow_call]'),
            W + 'on-map.yml': reusable('on-map', '{workflow_call: {}}'),
            W + 'plain.yml': reusable('plain', 'push'),
            # The hand declaration of #816, an existing flow: its sides and links stay at `declared`.
            'meada.toml': '''[[resource]]
kind = "gha-workflow"
id = "acme/workflows/.github/workflows/publish.yml"
role = "write"
via = [".github/workflows/publish.yml#on.workflow_call"]
''',
        },
        'wrapper': {W + 'release.yml': '''name: release
on:
  workflow_call:
    inputs:
      package:
        type: string
        required: true
jobs:
  # case: w_release
  # control: c_release
  release:
    permissions:
      id-token: write
      contents: read
    uses: acme/workflows/.github/workflows/publish.yml@main
    with:
      package: ${{ inputs.package }}
'''},
        'app-a': {
            W + 'ci.yml': '''name: ci
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      # case: a_checkout
      # control: step_remote_root
      - id: a_checkout
        uses: actions/checkout@v4
      # case: a_image
      # control: step_remote_folder
      - id: a_image
        name: Build neutral image
        uses: docker/build-push-action@v6
        env:
          CHANNEL_ENV: neutral-env
        with:
          context: .
          build-args: CHANNEL=neutral-zz
          tags: ghcr.io/acme/app-a:latest
  # case: a_publish
  # control: c_release
  publish:
    needs: build
    permissions:
      id-token: write
      contents: read
    uses: acme/workflows/.github/workflows/publish.yml@main
    with:
      package: app-a
''',
            'meada.toml': '''[[resource]]
kind = "gha-workflow"
id = "acme/workflows/.github/workflows/publish.yml"
role = "read"
via = [".github/workflows/ci.yml#jobs.publish.uses"]
''',
        },
        'app-b': {W + 'ci.yml': '''name: ci
on: push
permissions:
  contents: read
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      # case: b_checkout
      # control: step_remote_root
      - id: b_checkout
        uses: actions/checkout@v4
      # case: b_image
      # control: step_remote_folder
      - id: b_image
        uses: docker/build-push-action@v6
        with:
          context: .
  # case: b_publish_prd
  # control: b_publish_stg
  publish-prd:
    uses: acme/workflows/.github/workflows/publish.yml@v1
    with:
      package: app-b
  # case: b_publish_stg
  # control: b_publish_prd
  publish-stg:
    permissions:
      id-token: write
      contents: read
    uses: acme/workflows/.github/workflows/publish.yml@main
    with:
      package: app-b-stg
'''},
        'app-c': {W + 'ci.yml': '''name: ci
on: push
jobs:
  # case: c_release
  # control: a_publish
  release:
    uses: acme/wrapper/.github/workflows/release.yml@main
    with:
      package: app-c
'''},
        'actions': {
            'action.yml': COMPOSITE.replace('NAME', 'root'),
            'setup/action.yml': COMPOSITE.replace('NAME', 'setup'),
            'setup/action.yaml': COMPOSITE.replace('NAME', 'setup-shadowed'),
            'only-yaml/action.yaml': COMPOSITE.replace('NAME', 'only-yaml'),
            'docker-only/Dockerfile': 'FROM alpine:3.20\n',
        },
        'forms': {
            W + 'forms.yml': '''name: forms
on: push
jobs:
  # case: job_remote
  # control: job_external
  job_remote:
    uses: acme/workflows/.github/workflows/on-string.yml@v1
    with:
      alpha: 1
      beta: two
    secrets: inherit
  # case: job_external
  # control: job_remote
  job_external:
    uses: other-org/workflows/.github/workflows/publish.yml@main
  # case: job_local
  # control: job_local_missing
  job_local:
    uses: ./.github/workflows/local.yml
  # case: job_local_missing
  # control: job_local
  job_local_missing:
    uses: ./.github/workflows/missing.yml
  # case: job_dollar
  # control: job_local_missing
  job_dollar:
    uses: $/.github/workflows/local.yml
  # case: job_owner_case
  # control: job_path_case
  job_owner_case:
    uses: Acme/Workflows/.github/workflows/on-list.yml@main
  # case: job_path_case
  # control: job_owner_case
  job_path_case:
    uses: acme/workflows/.github/workflows/On-List.yml@main
  # case: job_on_map
  # control: job_not_reusable
  job_on_map:
    uses: acme/workflows/.github/workflows/on-map.yml@main
    secrets:
      token: ${{ secrets.TOKEN }}
  # case: job_not_reusable
  # control: job_on_map
  job_not_reusable:
    uses: acme/workflows/.github/workflows/plain.yml@main
  steps_own_checkout:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # case: step_remote_root
      # control: step_remote_folder
      - id: step_remote_root
        uses: acme/actions@v1
      # case: step_remote_folder
      # control: step_only_yaml
      - id: step_remote_folder
        uses: acme/actions/setup@v1
        with:
          version: "3"
      # case: step_only_yaml
      # control: step_remote_folder
      - id: step_only_yaml
        uses: acme/actions/only-yaml@v1
      # case: step_docker_only
      # control: step_missing_folder
      - id: step_docker_only
        uses: acme/actions/docker-only@v1
      # case: step_missing_folder
      # control: step_docker_only
      - id: step_missing_folder
        uses: acme/actions/nowhere@v1
      # case: step_docker_ref
      # control: step_remote_folder
      - id: step_docker_ref
        uses: docker://alpine:3.20
      # case: step_external
      # control: step_remote_root
      - id: step_external
        uses: actions/setup-node@v4
      # case: step_local_own
      # control: step_local_no_checkout
      - id: step_local_own
        uses: ./local-action
      # case: step_dollar
      # control: step_local_no_checkout
      - id: step_dollar
        uses: $/local-action
  steps_no_checkout:
    runs-on: ubuntu-latest
    steps:
      # case: step_local_no_checkout
      # control: step_local_own
      - id: step_local_no_checkout
        uses: ./local-action
  steps_checkout_repository:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          repository: acme/app-a
      # case: step_local_other_repository
      # control: step_local_own
      - id: step_local_other_repository
        uses: ./local-action
  steps_checkout_path:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          path: src
      # case: step_local_checkout_path
      # control: step_local_own
      - id: step_local_checkout_path
        uses: ./local-action
''',
            W + 'local.yml': '''name: local
on: workflow_call
jobs:
  inner:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # case: step_local_in_reusable
      # control: step_dollar_in_reusable
      - id: step_local_in_reusable
        uses: ./local-action
      # case: step_dollar_in_reusable
      # control: step_local_in_reusable
      - id: step_dollar_in_reusable
        uses: $/local-action
''',
            'local-action/action.yml': '''name: local-action
runs:
  using: composite
  steps:
    # case: step_local_in_composite
    # control: step_dollar_in_composite
    - id: step_local_in_composite
      uses: ./local-action/nested
    # case: step_dollar_in_composite
    # control: step_local_in_composite
    - id: step_dollar_in_composite
      uses: $/local-action/nested
''',
            'local-action/nested/action.yml': COMPOSITE.replace('NAME', 'nested'),
            W + 'uses.yml': '''name: uses
on: push
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      # case: use_typed_key
      # control: use_other_file
      - id: use_typed_key
        uses: other-org/upload@v1
        with:
          bucket: acme-plain-artifacts
      # case: use_structured
      # control: use_typed_key
      - id: use_structured
        uses: other-org/upload@v1
        with:
          target: s3://acme-structured-artifacts/releases
      # case: use_role_arn
      # control: use_structured
      - id: use_role_arn
        uses: other-org/credentials@v1
        with:
          role-to-assume: arn:aws:iam::123456789012:role/acme-deploy
''',
            '.github/settings.yml': '''# case: use_other_file
# control: use_typed_key
storage:
  bucket: acme-settings-artifacts
''',
        },
    },
    'refs': {
        'ref-provider': {W + 'build.yml': reusable('build')},
        'ref-consumer': {W + 'ci.yml': f'''name: ci
on: push
jobs:
  # case: ref_equal
  # control: ref_differs
  ref_equal:
    uses: acme/ref-provider/.github/workflows/build.yml@{HEAD_REF}
  # A branch of this shape reads as a commit id too: the ceiling ADR 0028 lists.
  # case: ref_differs
  # control: ref_equal
  ref_differs:
    uses: acme/ref-provider/.github/workflows/build.yml@{OTHER_40}
  # case: ref_differs_64
  # control: ref_equal
  ref_differs_64:
    uses: acme/ref-provider/.github/workflows/build.yml@{OTHER_64}
  # case: ref_short
  # control: ref_differs
  ref_short:
    uses: acme/ref-provider/.github/workflows/build.yml@{SHORT}
'''},
    },
    'orphan': {
        'lonely': {W + 'unused.yml': '# case: orphan_unused\n# control: orphan_no_scope\n' + reusable('unused'),
                   W + 'used.yml': reusable('used')},
        'lonely-caller': {W + 'ci.yml': '''name: ci
on: push
jobs:
  # case: orphan_used
  # control: orphan_unused
  call:
    uses: acme/lonely/.github/workflows/used.yml@main
'''},
    },
    # A scope without consumers: its provider is no orphan line.
    'orphan-control': {'unreferenced': {W + 'unused.yml': '# case: orphan_no_scope\n# control: orphan_unused\n' + reusable('unused')}},
    'solo': {
        'solo': {W + 'build.yml': reusable('build'), W + 'ci.yml': '''name: ci
on: push
jobs:
  # case: solo_local
  # control: solo_remote
  solo_local:
    uses: ./.github/workflows/build.yml
  # case: solo_remote
  # control: solo_local
  solo_remote:
    uses: acme/solo-peer/.github/workflows/build.yml@main
'''},
        'solo-peer': {W + 'build.yml': '# case: solo_peer_unused\n# control: orphan_no_scope\n' + reusable('build')},
    },
}
NO_ORIGIN = {'solo'}
# #820, "Further Notes": `context` on `publish.yml` lists exactly these (repository, file, address, ref).
PUBLISH_CONSUMERS = [
    (f'{HOST}/app-a', W + 'ci.yml', 'jobs.publish', '@main'),
    (f'{HOST}/app-b', W + 'ci.yml', 'jobs["publish-prd"]', '@v1'),
    (f'{HOST}/app-b', W + 'ci.yml', 'jobs["publish-stg"]', '@main'),
    (f'{HOST}/wrapper', W + 'release.yml', 'jobs.release', '@main'),
]


SLICES = {'f1': tuple(WORKSPACES), 'f2': ('run', 'chain'), 'f3': ('calls', 'permissions')}

# Slice F2 (#820, "The `run` script and the command table" and "Programs of the workspace"). The workspace
# `run` holds one CI job per case in `acme/steps`, named like its case, over the PyPI packages of `acme/libs`.
LIBS, STEPS, SDK = f'{HOST}/libs', f'{HOST}/steps', f'{HOST}/core-sdk'
# folder of `acme/libs` -> (distribution, version, its console script and the function it names)
PACKAGES = {
    'lib': ('acme-lib', '1.4.0', {}),
    'tool': ('acme-tool', '0.3.0', {}),
    'plain': ('acme-plain', '0.1.0', {'plain-cli': 'plain_main'}),  # no script named like the package
    'sync': ('acme-sync-kit', '2.0.0', {'acme-sync': 'sync_main'}),
    'fmt': ('acme-fmt', '1.0.0', {'acme-fmt-all': 'fmt_all_main', 'acme-fmt': 'fmt_main'}),  # `uvx` names the package: its own script
    'suite': ('acme-fast-suite', '1.0.0', {}),  # the one package the pattern of the forms fits
    'flavor': ('flavor', '0.1.0', {}),  # named like a variable of the forms: never matched by that name
    'dup-a': ('acme-dup-a', '1.0.0', {'acme-dup': 'dup_a_main'}),
    'dup-b': ('acme-dup-b', '1.0.0', {'acme-dup': 'dup_b_main'}),
}


def package(name):
    return f'package::pypi::{name}'


def manifest(name, version, scripts, more=''):
    """A `pyproject.toml` whose console scripts name functions of `<module>.cli`."""
    module = name.replace('-', '_')
    declared = ''.join(f'{script} = "{module}.cli:{function}"\n' for script, function in scripts.items())
    return f'[project]\nname = "{name}"\nversion = "{version}"\n{more}' + ('\n[project.scripts]\n' + declared if scripts else '')


def entry(folder):
    """The program the console script of a package of `acme/libs` names: its package and the site of its function."""
    name, _, scripts = PACKAGES[folder]
    function = scripts.get(name) or next(iter(scripts.values()))
    assert name in scripts or len(scripts) == 1, folder
    return name, f'{LIBS}::{folder}/{name.replace("-", "_")}/cli.py::{function}'


def libs():
    files = {}
    for folder, (name, version, scripts) in PACKAGES.items():
        module = f'{folder}/{name.replace("-", "_")}'
        files[f'{folder}/pyproject.toml'] = manifest(name, version, scripts)
        files[f'{module}/__init__.py'] = f'VERSION = "{version}"\n'
        if scripts:
            files[f'{module}/cli.py'] = ''.join(f'def {function}():\n    return 0\n' for function in scripts.values())
    return files


# The sides #820 gives a CI step. A consumer of a package carries the range as written; a program is the
# level of its link and the entry points it reaches; a builder is the image it is the site of.
def consumer(name, written=None):
    return 'consumer', package(name), written


def program(level, *folders):
    return 'program', level, tuple(entry(folder) for folder in folders)


def image(name):
    return 'builder', f'image::{name}'


def probe(control, script, *sides, **options):
    return control, script, sides, options


KO = lambda name: {'job': {'env': {'KO_DOCKER_REPO': f'ghcr.io/acme/{name}'}}}  # where the reader of today takes it
PWSH = {'defaults': {'run': {'shell': 'pwsh'}}}
# case: (control, the script of its CI step or the step's own keys, the sides of the CI step, options).
# Options: `shell` and `env` of the CI step, `job` (keys of its CI job), `before` and `after` (the package an
# earlier or a later CI step of the job installs with `pip install`), `file` (its pipeline, `run.yml` unless named).
PROBES = {
    # One per row of the command table.
    'pip_install': probe('pip_download', 'pip install acme-lib', consumer('acme-lib')),
    'pip3_install': probe('pip_download', 'pip3 install "acme-lib>=1.2,<2"', consumer('acme-lib', '>=1.2,<2')),
    'python_m_pip': probe('conda_install', 'python -m pip install acme-tool==0.3.0', consumer('acme-tool', '==0.3.0')),
    'uv_pip': probe('conda_install', 'uv pip install acme-lib', consumer('acme-lib')),
    'pipx_install': probe('conda_install', 'pipx install acme-tool', consumer('acme-tool')),
    'uv_tool': probe('pip_download', 'uv tool install acme-tool', consumer('acme-tool')),
    'console_script': probe('level_none', 'pip install acme-sync-kit\nacme-sync --all', consumer('acme-sync-kit'), program('exact', 'sync')),
    'docker_push': probe('docker_build', 'docker push ghcr.io/acme/pushed:1.0', image('ghcr.io/acme/pushed')),
    'buildx_push': probe('buildx_no_push', 'docker buildx build --push -t ghcr.io/acme/built:1.0 --tag ghcr.io/acme/built-alt:1.0 .',
                         image('ghcr.io/acme/built'), image('ghcr.io/acme/built-alt')),
    'docker_push_sha': probe('docker_build', 'docker push ghcr.io/acme/tagged:${{ github.sha }}', image('ghcr.io/acme/tagged')),  # the tag is no part of the name
    'ko_build': probe('ko_not_bare', 'ko build --bare ./cmd/app', image('ghcr.io/acme/ko-built'), **KO('ko-built')),
    'ko_publish': probe('ko_not_bare', 'ko publish --bare ./cmd/app', image('ghcr.io/acme/ko-published'), **KO('ko-published')),
    # The builder forms of actions, read before F2: they keep their rule and take the CI step as their site.
    'build_push_action': probe('docker_build', {'uses': 'docker/build-push-action@v6', 'with': {'push': True, 'tags': 'ghcr.io/acme/actioned:1.0'}},
                               image('ghcr.io/acme/actioned')),
    'metadata_action': probe('docker_build', {'uses': 'docker/metadata-action@v5', 'with': {'images': 'ghcr.io/acme/described'}},
                             image('ghcr.io/acme/described')),
    'docker_build': probe('docker_push', 'docker build -t ghcr.io/acme/local:1.0 .'),
    'buildx_no_push': probe('buildx_push', 'docker buildx build -t ghcr.io/acme/unpushed:1.0 .'),
    'ko_not_bare': probe('ko_build', 'ko build ./cmd/app', **KO('ko-plain')),
    # One per rule of the gate: units, the heredoc, where a command starts, and the text that is none.
    'two_lines': probe('heredoc', 'pip install acme-lib\npip install acme-tool', consumer('acme-lib'), consumer('acme-tool')),
    'heredoc': probe('two_lines', 'cat > notes.txt <<EOF\npip install acme-tool\nEOF\npip install acme-lib'),
    'echo': probe('pip_install', 'echo pip install acme-lib\necho "pip install acme-tool"'),
    'comment': probe('pip_install', '# pip install acme-lib\ntrue # pip install acme-tool'),
    'string': probe('and_list', 'git commit -m "pip install acme-lib && pip install acme-tool; pip install acme-lib"'),
    'and_list': probe('string', 'true && pip install acme-lib', consumer('acme-lib')),
    'semicolon': probe('string', 'cd src; pip install acme-tool', consumer('acme-tool')),
    'if_block': probe('echo', 'if [ -f setup.cfg ]; then\n  pip install acme-lib\nfi', consumer('acme-lib')),
    'loop': probe('echo', 'for attempt in 1 2 3; do\n  pip install acme-tool\ndone', consumer('acme-tool')),
    'continuation': probe('comment', 'pip install \\\n  acme-lib', consumer('acme-lib')),
    'quoted': probe('string', "pip install 'acme-tool==0.3.0'", consumer('acme-tool', '==0.3.0')),
    'then_same_line': probe('echo', 'if [ -f setup.cfg ]; then pip install acme-lib; fi', consumer('acme-lib')),
    'do_same_line': probe('echo', 'for attempt in 1 2 3; do pip install acme-tool; done', consumer('acme-tool')),
    # The gate names `&&` and `;` alone: after `||` or a pipe, or behind another command, a command is not read.
    'or_list': probe('and_list', 'false || pip install acme-lib'),
    'pipe': probe('and_list', 'yes | pip install acme-lib'),
    'prefix': probe('pip_install', 'sudo pip install acme-lib'),
    'two_names': probe('no_provider', 'pip install acme-lib acme-tool', consumer('acme-lib'), consumer('acme-tool')),
    # A package no repository of the workspace provides holds no side, as a dependency entry of a manifest.
    'no_provider': probe('pip_install', 'pip install acme-absent'),
    'conda_install': probe('pip_install', 'conda install acme-lib'),
    'pip_download': probe('pip_install', 'pip download acme-lib'),
    # No shell is evaluated: an assignment is not followed, and a literal `env` gives a value.
    'assignment': probe('env_literal', 'PKG=acme-lib\npip install $PKG', ('unresolved', 'PKG')),
    'env_literal': probe('assignment', 'pip install $PKG', consumer('acme-lib'), env={'PKG': 'acme-lib'}),
    'env_expression': probe('whole_vars', 'pip install ${{ env.TOOL }}', consumer('acme-tool'), job={'env': {'TOOL': 'acme-tool'}}),
    # The pipeline's `env` gives `WHICH`, and a nearer `env` that defines the name decides.
    'env_pipeline': probe('env_nearest', 'pip install $WHICH', consumer('acme-tool')),
    'env_nearest': probe('env_pipeline', 'pip install $WHICH', consumer('acme-lib'), env={'WHICH': 'acme-lib'}),
    # An argument that is no requirement by name gives no side (#831).
    'requirements_file': probe('pip_install', 'pip install -r requirements.txt'),
    'editable': probe('pip_install', 'pip install -e .'),
    'wheel_path': probe('pip_install', 'pip install ./dist/acme_lib-1.4.0-py3-none-any.whl'),
    'archive_url': probe('pip_install', 'pip install https://files.example.com/acme-lib-1.4.0.tar.gz'),
    'option_value': probe('archive_url', 'pip install --index-url https://pypi.example.com/simple acme-tool', consumer('acme-tool')),
    'option_names_package': probe('two_names', 'pip install --no-binary acme-lib acme-tool', consumer('acme-tool')),
    # The shell: the CI step's, else the `defaults.run.shell` of its CI job, else of its pipeline (#831).
    'shell_bash': probe('shell_pwsh', 'pip install acme-lib', consumer('acme-lib'), shell='bash'),
    'shell_sh': probe('shell_pwsh', 'pip install acme-tool', consumer('acme-tool'), shell='sh'),
    'shell_pwsh': probe('shell_bash', 'pip install acme-lib', shell='pwsh'),
    'shell_job_default': probe('shell_step_over_default', 'pip install acme-lib', job=PWSH),
    'shell_step_over_default': probe('shell_job_default', 'pip install acme-lib', consumer('acme-lib'), shell='bash', job=PWSH),
    'shell_pipeline_default': probe('shell_job_over_pipeline', 'pip install acme-lib', file='defaults.yml'),
    'shell_job_over_pipeline': probe('shell_pipeline_default', 'pip install acme-lib', consumer('acme-lib'), file='defaults.yml',
                                     job={'defaults': {'run': {'shell': 'sh'}}}),
    # One per row of the level table. No CI job but its own installs for `level_inferred`: an install in
    # another CI job, as in a later CI step, is no evidence (#832).
    'level_earlier_step': probe('level_inferred', 'acme-sync --all', program('exact', 'sync'), before=['acme-sync-kit'], file='levels.yml'),
    'level_uvx': probe('level_uvx_no_script', 'uvx acme-fmt --check .', program('exact', 'fmt'), file='levels.yml'),
    'level_pipx_run': probe('level_uvx_no_script', 'pipx run acme-fmt --check .', program('exact', 'fmt'), file='levels.yml'),
    'level_uvx_no_script': probe('level_uvx', 'uvx acme-plain', consumer('acme-plain'), file='levels.yml'),
    'level_inferred': probe('level_earlier_step', 'acme-sync --all', program('inferred', 'sync'), file='levels.yml'),
    'level_later_step': probe('level_earlier_step', 'acme-sync --all', program('inferred', 'sync'), after=['acme-sync-kit'], file='levels.yml'),
    'level_ambiguous': probe('level_inferred', 'acme-dup --fast', program('ambiguous', 'dup-a', 'dup-b'), file='levels.yml'),
    'level_none': probe('level_inferred', 'acme-unknown --fast', file='levels.yml'),
    'level_later_line': probe('console_script', 'acme-sync --all\npip install acme-sync-kit', consumer('acme-sync-kit'), program('inferred', 'sync'),
                              file='levels.yml'),
    'level_pipx_install': probe('level_none', 'pipx install acme-fmt\nacme-fmt --check .', consumer('acme-fmt'), program('exact', 'fmt'), file='levels.yml'),
}
NEW_BUILDERS = ('docker_push', 'docker_push_sha', 'buildx_push')  # the forms the base does not read
# Each unresolved form, with the variable its side names when it names one, in the three places of the gate:
# in part of a name (a pattern one package of the workspace fits), as the whole name, inside an option's value.
FORMS = {'secrets': ('${{ secrets.SECRET_FLAVOR }}', 'SECRET_FLAVOR'), 'vars': ('${{ vars.FLAVOR }}', 'FLAVOR'), 'github': ('${{ github.ref_name }}', None),
         'matrix': ('${{ matrix.flavor }}', None), 'variable': ('$SHELL_FLAVOR', 'SHELL_FLAVOR'), 'substitution': ('$(cat flavor.txt)', None),
         'backticks': ('`cat flavor.txt`', None)}
# A pattern no package fits holds no side, as a name no repository provides.
PROBES['part_nothing'] = probe('part_vars', 'pip install other-${{ vars.FLAVOR }}-kit', file='forms.yml')
for form, (text, variable) in FORMS.items():
    more = {'file': 'forms.yml', **({'job': {'strategy': {'matrix': {'flavor': ['fast', 'slow']}}}} if form == 'matrix' else {})}
    PROBES[f'part_{form}'] = probe(f'whole_{form}', f'pip install acme-{text}-suite', ('pattern', package('acme-*-suite')), **more)
    PROBES[f'whole_{form}'] = probe(f'option_{form}', f'pip install {text}', ('unresolved', variable), **more)
    PROBES[f'option_{form}'] = probe(f'part_{form}', f'pip install --index-url https://{text}.example.com/simple acme-tool', consumer('acme-tool'), **more)
PIPELINES = {'run.yml': 'name: run\non: push\nenv:\n  WHICH: acme-tool\njobs:\n', 'forms.yml': 'name: forms\non: push\njobs:\n', 'levels.yml': 'name: levels\non: push\njobs:\n',
             'defaults.yml': 'name: defaults\non: push\ndefaults:\n  run:\n    shell: pwsh\njobs:\n'}


class Block(str):
    """A script of several lines, written as a YAML block so the generated pipeline reads as a person wrote it."""


yaml.add_representer(Block, lambda dumper, text: dumper.represent_scalar('tag:yaml.org,2002:str', text, style='|'), Dumper=yaml.SafeDumper)


def job_text(case, control, body):
    """A CI job named like its case, under the two marker lines."""
    text = yaml.safe_dump({case: body}, sort_keys=False, default_flow_style=False, width=1000)
    return f'  # case: {case}\n  # control: {control}\n' + ''.join(f'  {line}\n' for line in text.splitlines())


def pipelines():
    files = dict(PIPELINES)
    install = lambda name: {'run': f'pip install {name}'}
    for case, (control, script, _, options) in PROBES.items():
        step = dict(script) if isinstance(script, dict) else {'run': Block(script + '\n') if '\n' in script else script}
        step = {'id': case, **{key: options[key] for key in ('shell', 'env') if key in options}, **step}
        steps = [*map(install, options.get('before', [])), step, *map(install, options.get('after', []))]
        files[options.get('file', 'run.yml')] += job_text(case, control, {'runs-on': 'ubuntu-latest', **options.get('job', {}), 'steps': steps})
    return {W + file: text for file, text in files.items()}


def unmarked(text):
    return ''.join(line + '\n' for line in text.splitlines() if not re.fullmatch(r'\s*# (case|control): \w+', line))


INSTALL_ACTION = '''name: install
runs:
  using: composite
  steps:
    # case: composite_run
    # control: composite_pwsh
    - id: composite_run
      run: pip install acme-lib
      shell: bash
    # case: composite_pwsh
    # control: composite_run
    - id: composite_pwsh
      run: pip install acme-tool
      shell: pwsh
'''
WORKSPACES['run'] = {'libs': libs(), 'steps': {**pipelines(), 'tools/install/action.yml': INSTALL_ACTION}}
CASES.update({case: ('run', 'steps', W + options.get('file', 'run.yml'), case, case, ('run', *sides)) for case, (_, _, sides, options) in PROBES.items()})
CASES['composite_run'] = ('run', 'steps', 'tools/install/action.yml', 'runs', 'composite_run', ('run', consumer('acme-lib')))
CASES['composite_pwsh'] = ('run', 'steps', 'tools/install/action.yml', 'runs', 'composite_pwsh', ('run',))
# The chain of F2 (#820, "Further Notes"), over the callers of `calls`: `acme/core-api` serves a route,
# the console script of `acme/core-sdk` calls it with a base URL read from the environment (a link read
# from the SDK's code, today), and `publish.yml` installs the package and runs the script.
WORKSPACES['chain'] = {
    **{repo: {W + file: unmarked(WORKSPACES['calls'][repo][W + file])}
       for repo, file in (('app-a', 'ci.yml'), ('app-b', 'ci.yml'), ('app-c', 'ci.yml'), ('wrapper', 'release.yml'))},
    'workflows': {W + 'publish.yml': '''name: publish
on:
  workflow_call:
    inputs:
      package:
        type: string
        required: true
permissions:
  id-token: write
  contents: read
jobs:
  publish:
    runs-on: ubuntu-latest
    env:
      CORE_API_URL: ${{ vars.CORE_API_URL }}   # an organization variable: unresolved in CI
    steps:
      # case: chain_install
      # control: chain_run
      - id: chain_install
        run: pip install core-sdk
      # case: chain_run
      # control: chain_install
      - id: chain_run
        run: core-publish "${{ inputs.package }}"
'''},
    'core-api': {'app/main.py': '''from fastapi import FastAPI

app = FastAPI()


@app.post("/v1/packages")
def register_package():
    return {"registered": True}


@app.get("/v1/health")
def service_health():
    return {"ok": True}
'''},
    'core-sdk': {
        'pyproject.toml': manifest('core-sdk', '1.0.0', {'core-publish': 'publish_main', 'core-health': 'health_main'}, 'dependencies = ["requests"]\n'),
        'core_sdk/__init__.py': 'VERSION = "1.0.0"\n',
        'core_sdk/cli.py': '''import os
import sys

import requests

BASE = os.environ["CORE_API_URL"]


def publish_main():
    return requests.post(f"{BASE}/v1/packages", json={"name": sys.argv[1]})


def health_main():
    return requests.get(f"{BASE}/v1/health")
''',
    },
}
# What a peer entry holds beside its repository and path: the variable the SDK reads its base URL from.
PEER_KEYS = {('core-sdk', 'core-api'): 'base_url_env = ["CORE_API_URL"]\n'}
PUBLISH_MAIN, ROUTE = f'{SDK}::core_sdk/cli.py::publish_main', (f'{HOST}/core-api::app/main.py::register_package', 'http::POST::/v1/packages')
CASES.update({
    'chain_install': ('chain', 'workflows', W + 'publish.yml', 'publish', 'chain_install', ('run', consumer('core-sdk'))),
    'chain_run': ('chain', 'workflows', W + 'publish.yml', 'publish', 'chain_run', ('run', ('program', 'exact', (('core-sdk', PUBLISH_MAIN),)))),
})
# The site of every console script's function: a link from a CI step to one of them starts a program.
ENTRIES = {f'{LIBS}::{folder}/{name.replace("-", "_")}/cli.py::{function}' for folder, (name, _, scripts) in PACKAGES.items()
           for function in scripts.values()} | {PUBLISH_MAIN, f'{SDK}::core_sdk/cli.py::health_main'}
# The provider of a package: the site of its manifest.
MANIFESTS = {package(name): f'{LIBS}::{folder}/pyproject.toml' for folder, (name, _, _) in PACKAGES.items()} | {package('core-sdk'): f'{SDK}::pyproject.toml'}
CHAIN_CALLERS = [('app-a', 'jobs.publish'), ('app-b', 'jobs["publish-prd"]'), ('app-b', 'jobs["publish-stg"]'), ('wrapper', 'jobs.release'),
                 ('app-c', 'jobs.release')]

# Slice F3 (#820, "Declared permissions"). The key of a case is the block that governs its CI job as #820
# reads it, and the flags of its link: (permission, the level a CI job of the provider declares, the
# consumer's). The first five are the F3 columns of the table under "Further Notes".
BOTH = {'id-token': 'write', 'contents': 'read'}
DECLARED = {
    'a_publish': (BOTH, ()),
    'b_publish_prd': ({'contents': 'read'}, (('id-token', 'write', 'none'),)),
    'b_publish_stg': (BOTH, ()),
    'w_release': (BOTH, ()),
    'c_release': (NOT_DECLARED, ()),
    'perm_pipeline_block': ({'contents': 'read'}, (('id-token', 'write', 'none'),)),
    'perm_job_replaces': ({'id-token': 'write'}, (('contents', 'read', 'none'),)),  # a block is closed
    'perm_job_enough': (BOTH, ()),
    'perm_read_all': ('read-all', (('contents', 'write', 'read'),)),
    'perm_write_all': ('write-all', ()),
    'perm_empty': ({}, (('contents', 'read', 'none'),)),
    'perm_provider_job_block': ({'contents': 'read'}, ()),  # the provider's own job block replaces its pipeline's
    'perm_provider_not_declared': ({'contents': 'read'}, ()),  # a provider that declares none is not compared
    'perm_any_provider_job': ({'contents': 'read'}, (('packages', 'write', 'none'),)),
    'perm_both_flags': ({'contents': 'read'}, (('id-token', 'write', 'none'),)),  # with `ref mismatch`
    'perm_not_declared': (NOT_DECLARED, ()),
    'perm_job_alone': (BOTH, ()),
    'perm_two_flags': ({'packages': 'read'}, (('contents', 'read', 'none'), ('id-token', 'write', 'none'))),
}
PERMISSION_NAMES = {'id-token', 'contents', 'packages'}
# case: (control, its pipeline in `acme/perm-consumer`, the CI job's own block or None, the pipeline it calls).
# `blocks.yml` declares `contents: read` for the pipeline; `bare.yml` declares nothing.
CALLERS = {
    'perm_pipeline_block': ('perm_job_enough', 'blocks.yml', None, 'token.yml@main'),
    'perm_job_replaces': ('perm_job_enough', 'blocks.yml', {'id-token': 'write'}, 'token.yml@main'),
    'perm_job_enough': ('perm_pipeline_block', 'blocks.yml', BOTH, 'token.yml@main'),
    'perm_read_all': ('perm_write_all', 'blocks.yml', 'read-all', 'writes.yml@main'),
    'perm_write_all': ('perm_read_all', 'blocks.yml', 'write-all', 'token.yml@main'),
    'perm_empty': ('perm_write_all', 'blocks.yml', {}, 'job-over.yml@main'),
    'perm_provider_job_block': ('perm_pipeline_block', 'blocks.yml', None, 'job-over.yml@main'),
    'perm_provider_not_declared': ('perm_any_provider_job', 'blocks.yml', None, 'open.yml@main'),
    'perm_any_provider_job': ('perm_provider_job_block', 'blocks.yml', None, 'two-jobs.yml@main'),
    'perm_both_flags': ('perm_job_enough', 'blocks.yml', None, f'token.yml@{OTHER_40}'),
    'perm_not_declared': ('perm_pipeline_block', 'bare.yml', None, 'token.yml@main'),
    'perm_job_alone': ('perm_not_declared', 'bare.yml', BOTH, 'token.yml@main'),
    'perm_two_flags': ('perm_job_enough', 'blocks.yml', {'packages': 'read'}, 'token.yml@main'),
}


def called(name, pipeline=None, **jobs):
    """A reusable pipeline of `acme/perm-provider`: its own block or none, and the block of each CI job or none."""
    doc = {'name': name, 'on': 'workflow_call', **({'permissions': pipeline} if pipeline is not None else {}),
           'jobs': {job: {'runs-on': 'ubuntu-latest', **({'permissions': block} if block is not None else {}), 'steps': [{'run': f'echo {name}'}]}
                    for job, block in jobs.items()}}
    return yaml.safe_dump(doc, sort_keys=False, default_flow_style=False)


def callers(file):
    head = f'name: {file[:-4]}\non: push\n' + ('permissions:\n  contents: read\n' if file == 'blocks.yml' else '') + 'jobs:\n'
    return head + ''.join(job_text(case, control, {**({'permissions': block} if block is not None else {}), 'uses': f'acme/perm-provider/{W}{target}'})
                          for case, (control, where, block, target) in CALLERS.items() if where == file)


WORKSPACES['permissions'] = {
    'perm-provider': {
        W + 'token.yml': called('token', BOTH, run=None),
        W + 'writes.yml': called('writes', run={'contents': 'write'}),
        W + 'job-over.yml': called('job-over', {'id-token': 'write'}, run={'contents': 'read'}),
        W + 'open.yml': called('open', run=None),
        W + 'two-jobs.yml': called('two-jobs', first={'contents': 'read'}, second={'packages': 'write'}),
        'act/action.yml': COMPOSITE.replace('NAME', 'act'),
    },
    'perm-consumer': {
        # Only a link whose consumer is a CI job compares permissions: an action declares none.
        W + 'blocks.yml': callers('blocks.yml') + '''  perm_action:
    runs-on: ubuntu-latest
    steps:
      # case: perm_action_step
      # control: perm_job_alone
      - id: perm_action_step
        uses: acme/perm-provider/act@v1
''',
        W + 'bare.yml': callers('bare.yml'),
    },
}
CASES.update({case: ('permissions', 'perm-consumer', W + file, case, None, ('permissions', *DECLARED[case])) for case, (_, file, _, _) in CALLERS.items()})
CASES['perm_action_step'] = ('permissions', 'perm-consumer', W + 'blocks.yml', 'perm_action', 'perm_action_step', ('permissions', None, ()))


def governing(doc, job):
    """The block that governs a CI job: its own, else its pipeline's, else `not declared` (#820)."""
    body = doc['jobs'][job]
    return body['permissions'] if 'permissions' in body else doc.get('permissions', NOT_DECLARED)


def held(block, name):
    """The level a block holds for a permission. A block is closed: one it does not name is `none`."""
    return {'read-all': 'read', 'write-all': 'write'}.get(block) if isinstance(block, str) else block.get(name, 'none')


def short(consumer_block, provider):
    """What a CI job of the provider declares above the consumer's block: (permission, its level, the consumer's)."""
    found = set()
    for job in provider['jobs'] if consumer_block != NOT_DECLARED else ():
        block = governing(provider, job)
        if isinstance(block, dict):  # `not declared` is not compared, and no provider of the cases writes a shorthand
            found |= {(name, level, held(consumer_block, name)) for name, level in block.items()
                      if LEVELS.index(level) > LEVELS.index(held(consumer_block, name))}
    return tuple(sorted(found))


def declared(case):
    """This driver's reading of a case of F3: the block that governs its CI job and the flags of its link."""
    workspace, repo, file, job, _, _ = CASES[case]
    doc = read(WORKSPACES[workspace][repo][file])
    _, provider, path = doc['jobs'][job]['uses'].split('@')[0].split('/', 2)
    return governing(doc, job), short(governing(doc, job), read(WORKSPACES[workspace][provider][path]))


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def dump(path, data):
    write(path, json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True) + '\n')


def identity(repo):
    return SOLO if repo in NO_ORIGIN else f'{HOST}/{repo}'


def seed(root, workspaces):
    """Writes each of `workspaces` under `root` as sibling repositories with a hub, each committed once."""
    heads = {}
    for workspace in workspaces:
        repos = WORKSPACES[workspace]
        names = [*repos, 'hub']
        for name in names:
            path = root / workspace / name
            path.mkdir(parents=True)
            git('-c', 'init.defaultBranch=main', 'init', '-q', cwd=path)
            if name not in NO_ORIGIN:
                git('remote', 'add', 'origin', f'https://{HOST}/{name}.git', cwd=path)
            files = dict(repos.get(name, {}))
            # A repository without `origin` has no identity a peer entry could name: nobody lists it.
            peers = ''.join(f'[[peer]]\nrepo = "{HOST}/{other}"\npath = "../{other}"\n{PEER_KEYS.get((name, other), "")}\n'
                            for other in names if other not in (name, 'hub', *NO_ORIGIN))
            files['meada.toml'] = peers + files.get('meada.toml', '')
            for file, text in files.items():
                write(path / file, text.replace(HEAD_REF, heads.get('ref-provider', HEAD_REF)))
            git('add', '-A', cwd=path)
            git('commit', '-q', '-m', 'ci proof input', cwd=path)
            heads[name] = git('rev-parse', 'HEAD', cwd=path).strip()
    return heads


def key_step(key):
    return '.' + key if re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', key) else f'[{json.dumps(key)}]'


def read(text):
    """The first YAML document of a file, as #820 reads it."""
    return next(iter(yaml.safe_load_all(text)), None)


def node(doc, job, step):
    """This driver's reading of a case's node: its address and its mapping."""
    if job == 'runs':
        address, body = 'runs', doc['runs']
    else:
        address, body = 'jobs' + key_step(job), doc['jobs'][job]
    if step is None:
        return address, body
    index = [item.get('id') for item in body['steps']].index(step)
    return f'{address}.steps[{index}]', body['steps'][index]


def located(case):
    """The site and the mapping of a case, from the generated text."""
    workspace, repo, file, job, step, _ = CASES[case]
    if job is None:
        return f'{identity(repo)}::{file}::.', {}
    address, body = node(read(WORKSPACES[workspace][repo][file]), job, step)
    return f'{identity(repo)}::{file}::{address}', body


def fields(body):
    """The Git ref and the keys a consumer wrote, as the export carries them (#824: keys sorted)."""
    uses = str(body.get('uses', ''))
    row = {}
    if '@' in uses and not uses.startswith(('./', '$/', 'docker://')):
        row[REF] = uses.rsplit('@', 1)[1]
    if body.get('with'):
        row[WITH] = sorted(body['with'])
    if body.get('secrets'):
        row[SECRETS] = body['secrets'] if body['secrets'] == 'inherit' else sorted(body['secrets'])
    return row


def owned_files(files):
    """The pipelines and actions among `files` (path -> text) by this driver's reading: path -> document."""
    found = {}
    for file, text in files.items():
        name = Path(file).name
        try:
            doc = read(text) if file.startswith(W) and name.endswith(('.yml', '.yaml')) or name in ('action.yml', 'action.yaml') else None
        except yaml.YAMLError:
            continue
        if not isinstance(doc, dict):
            continue
        if file.startswith(W) and file.count('/') == 2 and 'jobs' in doc:
            found[file] = doc
        elif name in ('action.yml', 'action.yaml') and 'runs' in doc:
            if name == 'action.yml' or str(Path(file).with_name('action.yml')) not in files:
                found[file] = doc  # with both names, the action is `action.yml`
    return found


def ci_files(workspace):
    return {(repo, file): doc for repo, files in WORKSPACES[workspace].items() for file, doc in owned_files(files).items()}


def nodes(workspace):
    """Every CI node identity of a workspace: the file, its CI jobs and its CI steps."""
    out = set()
    for (repo, file), doc in ci_files(workspace).items():
        base = f'{identity(repo)}::{file}::'
        out.add(base + '.')
        blocks = [('jobs' + key_step(job), body) for job, body in doc.get('jobs', {}).items()] if 'jobs' in doc else [('runs', doc['runs'])]
        for address, body in blocks:
            if 'jobs' in doc:
                out.add(base + address)
            out.update(f'{base}{address}.steps[{index}]' for index in range(len(body.get('steps') or [])))
    return out


def providers(workspace):
    """Every provider side by this driver's reading: a pipeline with `workflow_call`, and an action."""
    out = set()
    for (repo, file), doc in ci_files(workspace).items():
        if 'jobs' in doc:
            on = doc.get(True, doc.get('on'))  # YAML 1.1 reads the key `on` as a boolean
            if on == 'workflow_call' or isinstance(on, (list, dict)) and 'workflow_call' in on:
                out.add((f'ci::{identity(repo)}/{file}', f'{identity(repo)}::{file}::.'))
        else:
            folder = str(Path(file).parent)
            out.add((f'ci::{identity(repo)}' + ('' if folder == '.' else f'/{folder}'), f'{identity(repo)}::{file}::.'))
    return out


def problems(cases, workspaces):
    """What makes a case unusable: no marker, no control, or a control that cannot fail."""
    found = []
    marked = {}
    for workspace, repos in workspaces.items():
        for repo, files in repos.items():
            for file, text in files.items():
                lines = text.splitlines()
                for number, line in enumerate(lines):
                    match = re.fullmatch(r'\s*# case: (\w+)', line)
                    if not match:
                        continue
                    control = re.fullmatch(r'\s*# control: (\w+)', lines[number + 1]) if number + 1 < len(lines) else None
                    if match[1] in marked:
                        found.append(f'{match[1]}: marked twice')
                    marked[match[1]] = (workspace, repo, file, control and control[1])
    for case, (workspace, repo, file, job, step, expected) in cases.items():
        if case not in marked:
            found.append(f'{case}: no `# case:` line in a generated file')
            continue
        if marked[case][:3] != (workspace, repo, file):
            found.append(f'{case}: marked in {marked[case][2]}, not in {file}')
        control = marked[case][3]
        if control is None:
            found.append(f'{case}: no `# control:` line')
        elif control not in cases:
            found.append(f'{case}: its control {control} is no case')
        elif cases[control][5] == expected:
            found.append(f'{case}: its control {control} expects the same answer, so it cannot fail')
    found += [f'{case}: marked in a file and missing from the table' for case in marked if case not in cases]
    return found


def selftest():
    assert not problems(CASES, WORKSPACES), problems(CASES, WORKSPACES)
    # Each defect is caught: a missing control line, and a control that expects the same answer.
    stripped = json.loads(json.dumps(WORKSPACES).replace('  # control: c_release\\n', '', 1))
    assert any('no `# control:` line' in line for line in problems(CASES, stripped))
    same = {**CASES, 'job_external': (*CASES['job_external'][:5], CASES['job_remote'][5])}
    assert any('cannot fail' in line for line in problems(same, WORKSPACES))
    assert any('no `# case:` line' in line for line in problems({**CASES, 'zz': CASES['a_publish']}, WORKSPACES))
    # The table of #820 against this driver's own reading of the files it generates.
    read_back = sorted((site.split('::')[0], site.split('::')[1], site.split('::')[2], '@' + fields(body)[REF])
                       for site, body in map(located, CASES) if body.get('uses', '').startswith('acme/workflows/' + W + 'publish.yml@'))
    assert read_back == sorted(PUBLISH_CONSUMERS), read_back
    for case, (workspace, _, _, job, _, expected) in CASES.items():
        site, body = located(case)
        assert job is None or expected[0].startswith(('use', 'no use')) or 'uses' in body or workspace in SLICES['f2'], case
        if expected[0] in ('link', 'mismatch') and expected[3]:
            assert fields(body)[REF] == expected[3], case
    assert fields(located('job_remote')[1]) == {REF: 'v1', WITH: ['alpha', 'beta'], SECRETS: 'inherit'}
    assert fields(located('job_on_map')[1]) == {REF: 'main', SECRETS: ['token']}
    assert located('b_publish_prd')[0] == f'{HOST}/app-b::{W}ci.yml::jobs["publish-prd"]'
    assert located('step_local_in_composite')[0] == f'{HOST}/forms::local-action/action.yml::runs.steps[0]'
    assert action('actions', 'setup') in providers('calls') and action('actions', 'only-yaml', 'action.yaml') in providers('calls')
    assert not any('plain.yml' in site or 'action.yaml::' in site and 'setup' in site or 'docker-only' in site for _, site in providers('calls'))
    assert {PUBLISH, RELEASE, LOCAL, LOCAL_ACTION, action('actions')} <= providers('calls')
    assert f'{HOST}/app-a::{W}ci.yml::jobs.build.steps[1]' in nodes('calls') and f'{HOST}/actions::action.yml::runs.steps[0]' in nodes('calls')
    # Slice F2: each script reads back from its generated pipeline, at the CI step the table places it.
    for case, (_, script, _, options) in PROBES.items():
        site, body = located(case)
        assert site == f'{STEPS}::{W}{options.get("file", "run.yml")}::jobs.{case}.steps[{len(options.get("before", []))}]', site
        assert not isinstance(script, str) or body['run'].rstrip('\n') == script and body.get('shell') == options.get('shell'), case
    assert {f'{place}_{form}' for place in ('part', 'whole', 'option') for form in FORMS} <= set(PROBES)
    # Past 64 CI jobs a pipeline loses the references to the rest (a ceiling of #823): no generated one comes near.
    assert all(len(doc.get('jobs', [])) <= 64 for workspace in SLICES['f2'] for doc in ci_files(workspace).values())
    for site in ENTRIES:  # every entry point is a function the generated files define
        repo, file, function = site.split('::')
        assert any(f'def {function}(' in files.get(file, '') for files in (WORKSPACES['run']['libs'], WORKSPACES['chain']['core-sdk'])), site
    assert [name for name, _, _ in PACKAGES.values() if re.fullmatch('acme-.+-suite', name)] == ['acme-fast-suite'], 'one package fits the pattern'
    assert not any(re.fullmatch('other-.+-kit', name) for name, _, _ in PACKAGES.values()), 'no package fits the control pattern'
    assert entry('fmt') == ('acme-fmt', f'{LIBS}::fmt/acme_fmt/cli.py::fmt_main') and f'{LIBS}::fmt/acme_fmt/cli.py::fmt_all_main' in ENTRIES
    assert located('composite_run')[0] == f'{STEPS}::tools/install/action.yml::runs.steps[0]' and located('composite_run')[1]['shell'] == 'bash'
    assert read(WORKSPACES['run']['steps'][W + 'run.yml'])['env'] == {'WHICH': 'acme-tool'}
    want = wanted('run')
    assert want[located('level_earlier_step')[0]][3] == {(package('acme-sync-kit'), entry('sync')[1], 'exact')}
    assert want[located('level_earlier_step')[0].replace('steps[1]', 'steps[0]')][0] == {package('acme-sync-kit')}
    assert want[located('level_later_step')[0].replace('steps[0]', 'steps[1]')][0] == {package('acme-sync-kit')}
    assert want[located('whole_vars')[0]][0] == {None} and want[located('heredoc')[0]] == (set(), set(), set(), set())
    assert fits(({package('acme-lib')}, set(), {(package('acme-lib'), f'{LIBS}::lib/pyproject.toml', 'exact')}), want[located('pip_install')[0]])
    assert not fits(({package('acme-lib')}, set(), {(package('acme-lib'), f'{LIBS}::lib/pyproject.toml', 'inferred')}), want[located('pip_install')[0]])
    assert not fits(({package('acme-lib')}, set(), set()), want[located('echo')[0]])
    assert not fits((set(), set(), {(package('acme-sync-kit'), entry('sync')[1], 'exact')}), want[located('level_inferred')[0]])
    assert not fits(({package('acme-lib')}, set(), {(package('acme-lib'), f'{LIBS}::tool/pyproject.toml', 'exact')}), want[located('pip_install')[0]])
    assert not fits(({package('acme-lib')}, set(), set()), want[located('pip_install')[0]]), 'a name the workspace provides links'
    pattern, part = package('acme-*-suite'), want[located('part_vars')[0]]
    assert fits(({pattern}, set(), {(pattern, f'{LIBS}::suite/pyproject.toml', 'ambiguous')}), part)
    assert not fits(({pattern}, set(), set()), part) and not fits(({pattern}, set(), {(pattern, f'{LIBS}::suite/pyproject.toml', 'exact')}), part)
    assert not fits(({pattern}, set(), {(pattern, f'{LIBS}::lib/pyproject.toml', 'ambiguous')}), part)
    assert not fits(({None, package('flavor')}, set(), {(package('flavor'), f'{LIBS}::flavor/pyproject.toml', 'exact')}), want[located('whole_vars')[0]])
    assert fits((set(), set(), {(package('acme-fmt'), entry('fmt')[1], 'exact')}), want[located('level_uvx')[0]])
    assert not fits((set(), set(), {(package('acme-fmt'), f'{LIBS}::fmt/acme_fmt/cli.py::fmt_all_main', 'exact')}), want[located('level_uvx')[0]])
    assert located('chain_run')[0] == f'{HOST}/workflows::{W}publish.yml::jobs.publish.steps[1]'
    assert 'case:' not in WORKSPACES['chain']['app-a'][W + 'ci.yml'] and 'publish.yml@main' in WORKSPACES['chain']['app-a'][W + 'ci.yml']
    # Slice F3: the keys of #820 against this driver's own reading of the blocks it generated.
    for case, key in DECLARED.items():
        assert declared(case) == key, (case, declared(case))
    assert {case for case, row in CASES.items() if row[0] == 'permissions'} == {*CALLERS, 'perm_action_step'}
    assert flagged('@v1 (indexed 0123456)') and flagged('@v1 (indexed 0123456), ref mismatch') and not flagged('@v1 (indexed 0123456) and more')
    assert flagged(f'@v1 (indexed 0123456), {PMISMATCH}: id-token, write against none')[2].startswith(', permissions')
    with tempfile.TemporaryDirectory(prefix='meada-ci-proof-') as tmp:
        heads = seed(Path(tmp), WORKSPACES)
        assert 'base_url_env = ["CORE_API_URL"]' in (Path(tmp) / 'chain/core-sdk/meada.toml').read_text()
        assert 'base_url_env' not in (Path(tmp) / 'chain/app-a/meada.toml').read_text()
        for workspace in WORKSPACES:  # every generated pipeline and action reads as YAML with its CI jobs or steps
            for (repo, file), doc in ci_files(workspace).items():
                assert read((Path(tmp) / workspace / repo / file).read_text().replace(heads['ref-provider'], HEAD_REF)) == doc, file
        text = (Path(tmp) / 'refs/ref-consumer' / W / 'ci.yml').read_text()
        assert f'build.yml@{heads["ref-provider"]}\n' in text and HEAD_REF not in text
        assert not (Path(tmp) / 'solo/solo/.git/refs/remotes').exists()
        assert 'path = "../app-c"' in (Path(tmp) / 'calls/hub/meada.toml').read_text()
        # Over an export with no row and answers with no line, each slice runs to its end and fails there:
        # a row the product does not give is a failed check, never a crash or a pass.
        for name, body in (('f1', f1), ('f2', f2), ('f3', f3)):
            run = Silent(Path(tmp) / 'silent' / name, None, None)
            body(run, heads, collections.defaultdict(dict), collections.defaultdict(str))
            failed = {check['name'] for check in run.checks if not check['passed']}
            assert failed, name
            for case, row in CASES.items():
                if name == 'f2' and row[0] in SLICES['f2'] and len(row[5]) > 1:
                    assert f'{case}: the sides of its CI step' in failed, case
                if name == 'f3' and case in DECLARED:
                    assert any(check.startswith(case + ': its link') for check in failed), case
    pinned = prove.infra()
    for name in CORPUS:
        assert re.fullmatch(r'[0-9a-f]{40}', pinned[name][2]) and pinned[name][1].startswith('https://github.com/'), name
    print(f'ci.py self-test: {len(CASES)} cases, each with a control that can fail')


class Run:
    def __init__(self, folder, head, base):
        self.folder, self.bins, self.checks, self.asked = folder, {'head': head, 'base': base}, [], []

    def check(self, name, ok, evidence):
        self.checks.append({'name': name, 'passed': bool(ok), 'evidence': evidence})
        dump(self.folder / 'checks.json', self.checks)

    def command(self, side, cwd, label, *words):
        result = subprocess.run([self.bins[side], *words], cwd=cwd, env={**os.environ, 'LANG': 'en_US.UTF-8'},
                                text=True, capture_output=True, timeout=600)
        write(self.folder / side / 'captures' / (label + '.stdout'), result.stdout)
        write(self.folder / side / 'captures' / (label + '.stderr'), result.stderr)
        result.check_returncode()
        return result.stdout

    def ask(self, side, cwd, label, queries):
        """The MCP answers of one stdio server started in `cwd`, in the order of `queries`."""
        log = self.folder / side / 'captures' / (label + '.mcp.err')
        log.parent.mkdir(parents=True, exist_ok=True)
        _, answers, _ = mcp_answers(self.bins[side], cwd, [{'tool': tool, 'arguments': arguments} for tool, arguments in queries], log)
        dump(log.with_suffix('.answers.json'), [{'tool': tool, 'arguments': arguments, 'answer': text}
                                                for (tool, arguments), text in zip(queries, answers)])
        self.asked.append((side, Path(cwd), queries, answers))
        return answers


class Silent(Run):
    """A run whose servers answer nothing, for the self-test."""

    def ask(self, side, cwd, label, queries):
        self.asked.append((side, Path(cwd), queries, [''] * len(queries)))
        return [''] * len(queries)


def file_of(site):
    """The file of a site written in the IaC node form, `<repository>::<file>::<address>`."""
    parts = (site or '').split('::')
    return parts[1] if len(parts) == 3 else None


def owned(section, row, own_files):
    """A row the spec lets differ from the base: a `ci::` row, a CI node, a use read from a CI file and its
    provisioning link, a consumer or a link sited on a CI node (an unresolved call, and from F2 the sides a
    `run` script gives) and, from F2, an image builder read from a CI file, whose site becomes its CI step.
    The slice's own checks say which of those rows must exist; any other row is the base's."""
    contract, read_from = row.get('contract') or '', (row.get('at') or '').split('#')[0]
    return (contract.startswith('ci::') or section in ('uses', 'provisions') and read_from in own_files
            or section == 'iac_nodes' and file_of(row.get('identity')) in own_files
            or section == 'consumers' and file_of(row.get('site')) in own_files
            or section == 'links' and file_of(row.get('consumer')) in own_files
            or section == 'providers' and contract.startswith('image::') and (read_from in own_files or file_of(row.get('site')) in own_files))


def parity(run, name, base, head, own_files):
    """Everything outside the rows this spec predicts is the base's, byte for byte."""
    differing = []
    for section in sorted(set(base) | set(head)):
        left, right = base.get(section), head.get(section)
        if isinstance(left, list) and isinstance(right, list):
            left = [row for row in left if not (isinstance(row, dict) and owned(section, row, own_files))]
            right = [row for row in right if not (isinstance(row, dict) and owned(section, row, own_files))]
        if left != right:
            differing.append(section)
    run.check(f'{name}: the export outside the CI rows equals the base', not differing, differing)


def drive(run, http, name):
    heads = seed(run.folder / 'input', SLICES[name])
    for side in run.bins:
        shutil.copytree(run.folder / 'input', run.folder / side / 'work', symlinks=True)
    dump(run.folder / 'identity.json', {side: {'binary': binary, 'sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest()}
                                         for side, binary in run.bins.items()})
    exports, statuses = {}, {}
    for side in run.bins:
        for workspace in SLICES[name]:
            for repo in [*WORKSPACES[workspace], 'hub']:
                cwd = run.folder / side / 'work' / workspace / repo
                exports[side, workspace, repo] = json.loads(run.command(side, cwd, f'{workspace}-{repo}-export', 'export'))
                statuses[side, workspace, repo] = run.command(side, cwd, f'{workspace}-{repo}-status', 'status')
    {'f1': f1, 'f2': f2, 'f3': f3}[name](run, heads, exports, statuses)
    for workspace in SLICES[name]:
        own_files = {file for _, file in ci_files(workspace)}
        for repo in [*WORKSPACES[workspace], 'hub']:
            parity(run, f'{workspace}/{repo}', exports['base', workspace, repo], exports['head', workspace, repo], own_files)
            write(run.folder / 'status' / f'{workspace}-{repo}.diff', '\n'.join(
                [f'- {line}' for line in statuses['base', workspace, repo].splitlines() if line not in statuses['head', workspace, repo].splitlines()]
                + [f'+ {line}' for line in statuses['head', workspace, repo].splitlines() if line not in statuses['base', workspace, repo].splitlines()]) + '\n')
        if http and workspace in REPLAYED:
            replay(run, workspace)


REPLAYED = ('calls', 'run', 'chain', 'permissions')


def replay(run, workspace):
    """MCP over HTTP: each stdio answer the head gave in `workspace`, asked again of one server. The server
    indexes the seeded repositories, which no index was written in: `meada index --workspace` records a
    commit only for a clean checkout, and a reason names the indexed commit."""
    folder, asked = run.folder / 'http' / workspace, run.folder / 'head' / 'work' / workspace
    names = [*WORKSPACES[workspace], 'hub']
    write(folder / 'workspace.toml', 'index_dir = "indexes"\n' + ''.join(
        f'\n[[repository]]\nrepo = "{HOST}/{name}"\npath = {json.dumps(str(run.folder / "input" / workspace / name))}\n' for name in names))
    run.command('head', folder, f'http-index-{workspace}', 'index', '--workspace', 'workspace.toml')
    port = free_port()
    with open(folder / 'server.log', 'w') as log:
        server = subprocess.Popen([run.bins['head'], 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                                  cwd=folder, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        write(folder / 'server.pid', str(server.pid))
        try:
            for _ in range(150):
                if server.poll() is not None:
                    raise RuntimeError('HTTP server exited')
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                    break
                except OSError:
                    time.sleep(.1)
            differing, replies = [], []
            for side, cwd, queries, answers in run.asked:
                if side != 'head' or cwd.parent != asked:
                    continue
                client = Http(f'http://127.0.0.1:{port}/{HOST}/{cwd.name}')
                for (tool, arguments), stdio in zip(queries, answers):
                    reply = answer(client.call('tools/call', {'name': tool, 'arguments': arguments}))
                    replies.append({'repository': cwd.name, 'tool': tool, 'arguments': arguments, 'answer': reply})
                    if reply != stdio:
                        differing.append(replies[-1])
            dump(folder / 'answers.json', replies)
            run.check(f'MCP over HTTP: each of the {len(replies)} answers of the `{workspace}` workspace byte-equal to stdio', replies and not differing, differing)
        finally:
            server.terminate()
            server.wait(timeout=20)


def f1(run, heads, exports, statuses):
    solo = exports['head', 'solo', 'solo'].get('repository')
    run.check('solo: the export names its repository', isinstance(solo, str) and solo, solo)
    solo = solo if isinstance(solo, str) and solo else SOLO

    def real(value):
        return json.loads(json.dumps(value).replace(SOLO, solo).replace(HEAD_REF, heads['ref-provider']))

    for workspace in SLICES['f1']:
        repos = WORKSPACES[workspace]
        own_files = {file for _, file in ci_files(workspace)}
        rows = lambda section: list({json.dumps(row, sort_keys=True): row for repo in repos
                                     for row in exports['head', workspace, repo].get(section, [])}.values())
        cases = {case: (real(located(case)[0]), located(case)[1], real(row[5])) for case, row in CASES.items() if row[0] == workspace}
        want_consumers, want_links = set(), set()
        for case, (site, body, expected) in cases.items():
            if expected[0] in ('link', 'mismatch', 'absent'):
                want_consumers.add((expected[1], site))
            if expected[0] == 'unresolved':
                want_consumers.add((None, site))
            if expected[0] in ('link', 'mismatch'):
                want_links.add((site, expected[2], expected[1], 'exact'))
        got_consumers = {(row['contract'], row['site']) for row in rows('consumers')
                         if (row['contract'] or '').startswith('ci::') or row['contract'] is None and file_of(row['site']) in own_files}
        got_links = {(row['consumer'], row['provider'], row['contract'], row['level']) for row in rows('links') if row['contract'].startswith('ci::')}
        got_providers = {(row['contract'], row['site']) for row in rows('providers') if row['contract'].startswith('ci::')}
        run.check(f'{workspace}: the whole set of CI providers', got_providers == set(map(tuple, real(sorted(providers(workspace))))), sorted(got_providers))
        run.check(f'{workspace}: the whole set of CI consumers', got_consumers == want_consumers,
                  {'missing': sorted(map(str, want_consumers - got_consumers)), 'unexpected': sorted(map(str, got_consumers - want_consumers))})
        run.check(f'{workspace}: the whole set of CI links', got_links == want_links,
                  {'missing': sorted(want_links - got_links), 'unexpected': sorted(got_links - want_links)})
        exported = {row['identity'] for row in rows('iac_nodes')}
        sided = {site for _, site in got_consumers | got_providers}
        run.check(f'{workspace}: every CI node that holds a side is in `iac_nodes`', sided <= exported, sorted(sided - exported))
        for case, (site, body, expected) in cases.items():
            workspace_, repo, file, job, step, _ = CASES[case]
            cwd = run.folder / 'head' / 'work' / workspace / repo
            if expected[0] in ('link', 'mismatch'):
                reasons = [row['reason'] for row in rows('links') if row['consumer'] == site and row['contract'] == expected[1]]
                if expected[3] is None:  # a local call and a `$/` call write no ref
                    ok = len(reasons) == 1 and '@' not in reasons[0] and 'ref mismatch' not in reasons[0]
                else:
                    indexed = heads[expected[2].split('::')[0].rsplit('/', 1)[1]]  # the provider's commit
                    shape = flagged(reasons[0]) if len(reasons) == 1 else None
                    ok = bool(shape) and shape[0] == expected[3] and indexed.startswith(shape[1]) and ('ref mismatch' in shape[2]) == (expected[0] == 'mismatch')
                run.check(f'{case}: the reason of its link', ok, reasons)
            if expected[0] in ('link', 'mismatch', 'absent'):
                row = [row for row in rows('consumers') if row['site'] == site and row['contract'] == expected[1]]
                got = {key: row[0].get(key) for key in (REF, WITH, SECRETS) if row and row[0].get(key) is not None}
                run.check(f'{case}: the Git ref and the keys it wrote', got == real(fields(body)), got)
                run.check(f'{case}: its consumer row {"is" if expected[0] == "absent" else "is not"} marked absent',
                          bool(row) and bool(row[0].get('absent')) == (expected[0] == 'absent'), row)
            if workspace == 'refs':
                reply = run.ask('head', cwd, f'{case}-context', [('context', {'symbol': site.split('::', 1)[1], 'budget': 200000})])[0]
                run.check(f'{case}: `context` on the consumer {"says" if expected[0] == "mismatch" else "does not say"} `ref mismatch`',
                          ('ref mismatch' in reply) == (expected[0] == 'mismatch') and f'@{expected[3]}' in reply, reply)
            if expected[0] in ('absent', 'unresolved') or case == 'step_docker_only':
                reply = run.ask('head', cwd, f'{case}-context', [('context', {'symbol': site.split('::', 1)[1], 'budget': 200000})])[0]
                run.check(f'{case}: {"marked" if expected[0] == "absent" else "not marked"} absent', (ABSENT in reply) == (expected[0] == 'absent'), reply)
            if expected[0] == 'unresolved':
                reply = run.ask('head', cwd, f'{case}-unresolved', [('search', {'query': step, 'kind': 'unresolved'})])[0]
                run.check(f'{case}: an open item of `search` kind `unresolved`', site.split('::')[2] in reply, reply)
        orphans = {match[1] for repo in [*repos, 'hub'] for line in statuses['head', workspace, repo].splitlines() if (match := ORPHAN.match(line))}
        want = {row[5][1] for row in CASES.values() if row[0] == workspace and row[5][0] == 'orphan'}
        run.check(f'{workspace}: the orphan lines of the CI kind', orphans == want, sorted(orphans))

    calls = lambda side, repo: run.folder / side / 'work' / 'calls' / repo
    big = {'budget': 200000}
    # External files, over the whole output: no side, no orphan line, no open item.
    everywhere = [*WORKSPACES['calls'], 'hub']
    named = [f'{repo}:{section}' for repo in everywhere for section, found in exports['head', 'calls', repo].items() if isinstance(found, list)
             for row in found if isinstance(row, dict) and any(name in str(row.get('contract')) for name in EXTERNAL)]
    run.check('external files name no contract in any export', not named, named)
    named = [repo for repo in [*WORKSPACES['calls'], 'hub'] if any(name in statuses['head', 'calls', repo] for name in EXTERNAL)]
    run.check('external files are in no line of `meada status`', not named, named)
    # A legitimate open item (a `./<folder>` step with no checkout) may say `checkout` in its cause: a line
    # counts only when it names the external file's own `uses`.
    open_items = [('search', {'query': query, 'kind': kind, **big}) for query in ('actions/checkout', 'build-push-action', 'setup-node', 'other-org', 'upload')
                  for kind in ('ambiguous', 'unresolved')]
    external_uses = ('actions/checkout@', 'actions/setup-node@', 'docker/build-push-action@', 'other-org/')
    for repo in everywhere:
        replies = run.ask('head', calls('head', repo), f'external-open-items-{repo}', open_items)
        run.check(f'{repo}: external files are no open item of `search`', not any(name in reply for reply in replies for name in external_uses), replies)
    reply = run.ask('head', calls('head', 'hub'), 'open-items-control', [('search', {'query': 'local-action', 'kind': 'unresolved', **big})])[0]
    run.check('control: `search` kind `unresolved` from the hub shows the open items of a peer', 'step_local_no_checkout' in reply or 'steps_no_checkout' in reply, reply)
    reply = run.ask('head', calls('head', 'hub'), 'external-control', [('search', {'query': 'actions/checkout', 'kind': 'value', **big})])[0]
    run.check('control: the `uses` lines of the external actions are read as config values', 'actions/checkout@v4' in reply, reply)

    # The reproduction of #816 from the hub: the consumers of `publish.yml` and the chain through the wrapper.
    hub_queries = [('context', {'symbol': PUBLISH[0], **big}), ('context', {'symbol': W + 'publish.yml', **big}),
                   ('trace', {'from': PUBLISH[0], 'direction': 'up', **big}), ('trace', {'from': pipeline('workflows', 'on-string.yml')[0], 'direction': 'up', **big})]
    hub = run.ask('head', calls('head', 'hub'), 'hub', hub_queries)
    for reply, label in zip(hub[:2], ('its contract', 'its file path')):
        listed = [row for row in PUBLISH_CONSUMERS if any(all(part in line for part in (*row, 'package')) for line in reply.splitlines())]
        run.check(f'`context` on `publish.yml` by {label} lists the four consumers, each on one line with its Git ref and keys',
                  listed == PUBLISH_CONSUMERS and f'{HOST}/app-c' not in reply and f'{HOST}/forms' not in reply, reply)
    reaches = lambda reply, repo, address: any(f'{HOST}/{repo}' in line and address in line for line in reply.splitlines())
    run.check('`trace` up from `publish.yml` reaches `app-c` through `wrapper`',
              reaches(hub[2], 'app-c', 'jobs.release') and reaches(hub[2], 'wrapper', 'jobs.release') and reaches(hub[2], 'app-a', 'jobs.publish'), hub[2])
    run.check('control: `trace` up from `on-string.yml` reaches `forms` and not `app-c`',
              reaches(hub[3], 'forms', 'jobs.job_remote') and f'{HOST}/app-c' not in hub[3], hub[3])
    up = run.ask('head', calls('head', 'workflows'), 'trace-file-path', [('trace', {'from': W + 'publish.yml', 'direction': 'up', **big})])[0]
    run.check('`trace` up from the file path of `publish.yml` reaches `app-c`', reaches(up, 'app-c', 'jobs.release'), up)
    up = run.ask('head', calls('head', 'wrapper'), 'trace-node', [('trace', {'from': W + 'release.yml::jobs.release', 'direction': 'up', **big})])[0]
    run.check('`trace` up from the CI job of `wrapper` reaches `app-c`', reaches(up, 'app-c', 'jobs.release'), up)
    down = run.ask('head', calls('head', 'app-c'), 'trace-down', [('trace', {'from': W + 'ci.yml::jobs.release', 'direction': 'down', **big})])[0]
    run.check('`trace` down from `app-c` reaches `release.yml` and `publish.yml`',
              f'{HOST}/wrapper' in down and 'release.yml' in down and f'{HOST}/workflows' in down and 'publish.yml' in down, down)

    # CI nodes and references, in `app-a` and `actions`.
    step_site = lambda case: located(case)[0]
    node_queries = [('search', {'query': 'publish', 'kind': 'ci'}), ('search', {'query': 'a_image', 'kind': 'ci'}),
                    ('search', {'query': 'docker/build-push-action', 'kind': 'ci'}), ('context', {'symbol': W + 'ci.yml', **big}),
                    ('context', {'symbol': W + 'ci.yml::jobs.publish', **big}), ('context', {'symbol': W + 'ci.yml::jobs.build', **big}),
                    ('context', {'symbol': W + 'ci.yml::jobs.build.steps[1]', **big}), ('context', {'symbol': W + 'ci.yml::jobs.build.steps[id=a_image]', **big})]
    wanted = [['jobs.publish', 'ci.yml'], ['jobs.build.steps[1]'], ['jobs.build.steps[1]'], ['jobs.build', 'jobs.publish'], ['jobs.build'],
              ['jobs.publish', 'jobs.build.steps[0]', 'jobs.build.steps[1]'], ['neutral-zz'], ['jobs.build.steps[1]', 'neutral-zz']]
    replies = run.ask('head', calls('head', 'app-a'), 'nodes', node_queries)
    for (tool, arguments), parts, reply in zip(node_queries, wanted, replies):
        run.check(f'{tool} {json.dumps(arguments)} shows {parts}', all(part in reply for part in parts), reply)
    by_type = run.ask('head', calls('head', 'hub'), 'nodes-by-type', [('search', {'query': kind, 'kind': 'ci', **big}) for kind in ('job', 'step')])
    for kind, reply in zip(('job', 'step'), by_type):
        want = [site for site in sorted(nodes('calls')) if ('.steps[' in site) == (kind == 'step') and not site.endswith('::.')]
        whole = lambda site, line: site.split('::')[1] in line and re.search(re.escape(site.split('::')[2]) + r'(?![\w.\[])', line)
        missing = [site for site in want if not any(whole(site, line) for line in reply.splitlines())]
        run.check(f'`search` kind `ci` for the type `{kind}` finds every CI {kind} this driver reads ({len(want)})', not missing, missing or reply)
    reply = run.ask('head', calls('head', 'actions'), 'composite', [('context', {'symbol': 'action.yml', **big})])[0]
    run.check('`context` on a composite action shows its own CI steps', 'runs.steps[0]' in reply, reply)
    named, callers = run.ask('head', calls('head', 'app-a'), 'step-name', [('search', {'query': 'Build neutral image', 'kind': 'ci'}),
                                                                           ('context', {'symbol': W + 'ci.yml::jobs.build.steps[1]', **big})])
    run.check('`search` kind `ci` finds a CI step by its `name`', 'jobs.build.steps[1]' in named, named)
    run.check('`context` on a CI step shows the values of its `env`', 'neutral-env' in callers, callers)
    setup = step_site('step_remote_folder').split('::')
    reply = run.ask('head', calls('head', 'actions'), 'action-callers', [('context', {'symbol': 'setup/action.yml', **big})])[0]
    run.check('`context` on an action lists the CI step that calls it', any(setup[0] in line and setup[2] in line for line in reply.splitlines()), reply)
    reply = run.ask('head', calls('head', 'forms'), 'trace-composite', [('trace', {'from': 'local-action/nested/action.yml', 'direction': 'up', **big})])[0]
    run.check('`trace` up from a nested action reaches the pipeline through the composite action that calls it',
              'runs.steps[1]' in reply and step_site('step_local_own').split('::')[2] in reply, reply)

    # The files the CI reader owns (#828): only the typed-key use goes, and an `exact` use gains its CI step.
    at = lambda side, suffix: [row for row in exports[side, 'calls', 'forms'].get('uses', []) if row['at'].endswith(suffix)]
    run.check('use_typed_key: no use from a plain name under a typed key', not at('head', '.with.bucket'), at('head', '.with.bucket'))
    run.check('use_role_arn: no use from a role ARN in `with`', not at('head', 'role-to-assume"]'), at('head', 'role-to-assume"]'))
    run.check('use_structured: the use is on its CI step', [row['site'] for row in at('head', '.with.target')] == [step_site('use_structured')], at('head', '.with.target'))
    run.check('use_other_file: another file of `.github/` keeps its use', len(at('head', 'storage.bucket')) == 1, at('head', 'storage.bucket'))

    run.check('control: the base gives the typed key a use', len(at('base', '.with.bucket')) == 1, at('base', '.with.bucket'))
    # IAM is in no row of the type table: the role ARN is no use on the base either (#828 recorded it).
    run.check('control: the base gives the role ARN no use either', not at('base', 'role-to-assume"]'), at('base', 'role-to-assume"]'))
    structured = [{key: value for key, value in row.items() if key != 'site'} for side in ('base', 'head') for row in at(side, '.with.target')]
    run.check('control: the base has the structured use on the repository, the same row but for its site',
              [row['site'] for row in at('base', '.with.target')] == [f'{HOST}/forms'] and structured[:1] == structured[1:], at('base', '.with.target'))
    run.check('use_other_file: equal to the base', at('head', 'storage.bucket') == at('base', 'storage.bucket'), at('base', 'storage.bucket'))
    # Existing answers: the `uses` lines as config values, the hand declaration, kind `iac` and the default search.
    same = [('search', {'query': 'publish.yml', 'kind': 'value', **big}), ('search', {'query': 'publish', 'kind': 'iac'}), ('search', {'query': 'publish'}),
            ('context', {'symbol': 'resource::gha-workflow::acme/workflows/.github/workflows/publish.yml', **big})]
    before, after = (run.ask(side, calls(side, 'hub'), 'existing', same) for side in ('base', 'head'))
    for query, left, right in zip(same, before, after):
        run.check(f'existing answer equal to the base: {json.dumps(query)}', left == right, [left, right])
    run.check('control: the base finds the four `uses` lines of `publish.yml` as config values', before[0].count('publish.yml@') >= 4, before[0])
    run.check('control: the base answers the hand declaration of #816 with its declared consumer', f'{HOST}/app-a' in before[3], before[3])


def gathered(exports, workspace, section, side='head'):
    """The rows of a section over the repositories of a workspace, each once."""
    return list({json.dumps(row, sort_keys=True): row for repo in WORKSPACES[workspace]
                 for row in exports[side, workspace, repo].get(section, [])}.values())


def wanted(workspace):
    """CI node -> (the contracts its consumer rows must hold, the ones they may hold, the images it is the
    site of, the programs it starts), by #820's tables. A program link goes through its package's contract,
    so the CI step may be a consumer of that package; a CI step that only installs is a consumer of its package."""
    want = {}
    for case, row in CASES.items():
        if row[0] != workspace or row[5][0] != 'run':
            continue
        sides, site = row[5][1:], located(case)[0]
        must = {side[1] for side in sides if side[0] in ('consumer', 'pattern')} | {None for side in sides if side[0] == 'unresolved'}
        programs = {(package(name), reached, side[1]) for side in sides if side[0] == 'program' for name, reached in side[2]}
        want[site] = (must, must | {contract for contract, _, _ in programs}, {side[1] for side in sides if side[0] == 'builder'}, programs)
        options = PROBES[case][3] if case in PROBES else {}
        before = options.get('before', [])
        for index, name in [*enumerate(before), *enumerate(options.get('after', []), len(before) + 1)]:
            want[f'{site.rsplit(".steps[", 1)[0]}.steps[{index}]'] = ({package(name)}, {package(name)}, set(), set())
    return want


def sided(rows, site):
    """What a CI node holds outside the CI kind: the contracts of its consumer rows, the images it is the
    site of, and its links as (contract, provider, level)."""
    other = lambda row: not (row['contract'] or '').startswith('ci::')
    return ({row['contract'] for row in rows('consumers') if row['site'] == site and other(row)},
            {row['contract'] for row in rows('providers') if row['site'] == site and other(row)},
            {(row['contract'], row['provider'], row['level']) for row in rows('links') if row['consumer'] == site and other(row)})


def fits(got, want):
    """A link to an entry point starts a program. A name the CI step installs links to the manifest of its
    package at `exact`: each package of the cases has one provider, in another repository (#820, "Programs
    of the workspace"). A pattern links to each package it fits, never at `exact`. The link of a package
    the CI step only starts a program of is allowed, to that manifest."""
    (consumers, builders, links), (must, may, images, programs) = got, want
    started = {link for link in links if link[1] in ENTRIES}
    names = {contract for contract in must if contract and '*' not in contract}
    patterns = {contract for contract in must if contract and '*' in contract}
    installs = {(contract, MANIFESTS[contract], 'exact') for contract in names}
    fitting = lambda pattern: {site for name, site in MANIFESTS.items() if re.fullmatch(re.escape(pattern).replace(r'\*', '.+'), name)}

    def other(link):
        contract, provider, level = link
        if contract in patterns:
            return level != 'exact' and provider in fitting(contract)
        return link in installs or contract in may - must and provider == MANIFESTS.get(contract)

    return (must <= consumers <= may and builders == images and started == programs and installs <= links
            and all(fitting(pattern) == {link[1] for link in links if link[0] == pattern} for pattern in patterns)
            and all(map(other, links - started)))


def f2(run, heads, exports, statuses):
    big = {'budget': 200000}
    at = lambda workspace, repo: run.folder / 'head' / 'work' / workspace / repo
    address = lambda case: located(case)[0].split('::')[2]
    for workspace in SLICES['f2']:
        rows = lambda section, workspace=workspace: gathered(exports, workspace, section)
        want = wanted(workspace)
        for case, row in CASES.items():
            if row[0] != workspace:
                continue
            site = located(case)[0]
            got = sided(rows, site)
            run.check(f'{case}: the sides of its CI step', fits(got, want[site]), [sorted(map(str, part)) for part in got])
            naming = json.dumps([found for section in ('consumers', 'links') for found in rows(section)
                                 if site in (found.get('site'), found.get('consumer'))], ensure_ascii=False)
            for side in row[5][1:]:
                if side[0] == 'consumer' and side[2]:
                    run.check(f'{case}: the range as written, `{side[2]}`', side[2] in naming, naming)
    # Silence, over the whole output of the `run` workspace: a CI node no case names holds no side.
    rows = lambda section: gathered(exports, 'run', section)
    want, nothing = wanted('run'), (set(), set(), set(), set())
    cases = {located(case)[0] for case, row in CASES.items() if row[0] == 'run'}
    wrong = [site for site in sorted(want) if site not in cases and not fits(sided(rows, site), want[site])]
    run.check('run: each CI step that only installs is a consumer of its package', not wrong, wrong)
    wrong = [site for site in sorted(nodes('run')) if site not in want and not fits(sided(rows, site), nothing)]
    run.check('run: no other CI node of the workspace holds a side', not wrong, wrong)
    exported = {row['identity'] for row in rows('iac_nodes')}
    wrong = [site for site in sorted(want) if any(sided(rows, site)) and site not in exported]
    run.check('run: every CI step that holds a side is in `iac_nodes`', not wrong, wrong)
    # An unresolved side names its variable, where the form holds one.
    variables = sorted({side[1] for row in CASES.values() if row[0] == 'run' for side in row[5][1:] if side[0] == 'unresolved' and side[1]})
    replies = dict(zip(variables, run.ask('head', at('run', 'steps'), 'unresolved-variables',
                                          [('search', {'query': name, 'kind': 'unresolved', **big}) for name in variables])))
    for case, row in CASES.items():
        for side in row[5][1:] if row[0] == 'run' else ():
            if side[0] == 'unresolved' and side[1]:
                run.check(f'{case}: `search` kind `unresolved` for `{side[1]}` shows its CI step', address(case) in replies[side[1]], replies[side[1]])
    # `context` on a package lists the CI steps that install it, and no CI step of a case that may not hold it.
    for name in ('acme-lib', 'acme-tool'):
        reply = run.ask('head', at('run', 'hub'), f'context-{name}', [('context', {'symbol': package(name), **big})])[0]
        listed = lambda case: any(STEPS in line and address(case) in line for line in reply.splitlines())
        installs = [case for case, row in CASES.items() if row[0] == 'run' and ('consumer', package(name)) in [side[:2] for side in row[5][1:]]]
        others = [case for case, row in CASES.items() if row[0] == 'run' and package(name) not in want[located(case)[0]][1]]
        run.check(f'`context` on `{package(name)}` lists each of the {len(installs)} CI steps that install it and none of the {len(others)} others',
                  all(map(listed, installs)) and not any(map(listed, others)), reply)
    # The image builder forms. The base reads the forms of today with the repository as their site, and
    # the only difference #820 allows is that site; `docker push` and `docker buildx build --push` are new.
    built = lambda side, contracts: sorted((row for row in exports[side, 'run', 'steps'].get('providers', []) if row['contract'] in contracts),
                                           key=lambda row: json.dumps(row, sort_keys=True))
    for case, row in CASES.items():
        contracts = {side[1] for side in row[5][1:] if side[0] == 'builder'} if row[0] == 'run' else set()
        if not contracts:
            continue
        before, after = built('base', contracts), built('head', contracts)
        if case in NEW_BUILDERS:
            run.check(f'control: the base reads no builder from {case}', not before, before)
            continue
        run.check(f'control: the base reads {case} with the repository as its site',
                  len(before) == len(contracts) and all(found['site'] == STEPS and found.get('at', '').startswith(f'{row[2]}#{address(case)}.') for found in before), before)
        run.check(f'{case}: the head row is the base row but for its site',
                  bool(before) and [{**found, 'site': located(case)[0]} for found in before] == after, after)
    # The chain: the HTTP link is read from the SDK's code and equals the base, and `trace` crosses the
    # three contracts both ways.
    links = lambda side: sorted(json.dumps(row, sort_keys=True) for row in gathered(exports, 'chain', 'links', side) if row['contract'].startswith('http::'))
    known = [row for row in map(json.loads, links('base'))
             if row['consumer'].startswith(PUBLISH_MAIN + '::') and (row['provider'], row['contract']) == ROUTE]
    run.check("control: the base links the SDK's program to the route of the API", len(known) == 1, links('base'))
    run.check('the HTTP links of the chain equal the base', links('head') == links('base'), links('head'))
    reaches = lambda reply, repo, word: any(f'{HOST}/{repo}' in line and word in line for line in reply.splitlines())
    up, control = run.ask('head', at('chain', 'core-api'), 'chain-up', [('trace', {'from': 'register_package', 'direction': 'up', **big}),
                                                                        ('trace', {'from': 'service_health', 'direction': 'up', **big})])
    missing = [f'{repo} {word}' for repo, word in [('core-sdk', 'publish_main'), ('workflows', address('chain_run')), *CHAIN_CALLERS] if not reaches(up, repo, word)]
    run.check("`trace` up from the route reaches the SDK's program, the CI step of `publish.yml` that runs it and the five calling CI jobs", not missing, missing or up)
    run.check('control: `trace` up from a route no CI step calls reaches the SDK and no pipeline',
              reaches(control, 'core-sdk', 'health_main') and f'{HOST}/workflows' not in control and f'{HOST}/app-' not in control, control)
    down = run.ask('head', at('chain', 'app-c'), 'chain-down', [('trace', {'from': W + 'ci.yml::jobs.release', 'direction': 'down', **big})])[0]
    missing = [f'{repo} {word}' for repo, word in [('wrapper', 'release.yml'), ('workflows', 'publish.yml'), ('core-sdk', 'publish_main'),
                                                   ('core-api', 'register_package')] if not reaches(down, repo, word)]
    run.check('`trace` down from `app-c` reaches both pipelines, the program and the route', not missing, missing or down)


def f3(run, heads, exports, statuses):
    big = {'budget': 200000}
    contexts = {}
    for case, (block, flags) in DECLARED.items():
        workspace, repo = CASES[case][:2]
        site, body = located(case)
        rows = lambda section, workspace=workspace: gathered(exports, workspace, section)
        contract = 'ci::github.com/' + body['uses'].split('@')[0]
        links = [row for row in rows('links') if row['consumer'] == site and row['contract'] == contract]
        shape = flagged(links[0]['reason']) if len(links) == 1 else None
        tail = shape[2] if shape else ''
        names = lambda word: re.search(r'(?<![\w-])' + re.escape(word) + r'(?![\w-])', tail)
        run.check(f'{case}: its link {"names" if flags else "carries no"} `{PMISMATCH}`',
                  bool(shape) and (PMISMATCH in tail) == bool(flags) and all(names(word) for flag in flags for word in flag)
                  and not any(names(name) for name in PERMISSION_NAMES - {flag[0] for flag in flags}), links)
        if case == 'perm_both_flags':
            run.check(f'{case}: its link carries `ref mismatch` too', 'ref mismatch' in tail, links)
        found = [row for row in rows('consumers') if row['site'] == site and row['contract'] == contract]
        if isinstance(block, dict) and block:
            text = json.dumps(found[0].get(PERMISSIONS)) if len(found) == 1 else ''
            run.check(f'{case}: its consumer row carries the block that governs it, under `{PERMISSIONS}`',
                      all(name in text and level in text for name, level in block.items()), found)
        if contract not in contexts:
            contexts[contract] = run.ask('head', run.folder / 'head' / 'work' / workspace / 'hub', f'permissions-context-{len(contexts)}',
                                         [('context', {'symbol': contract, **big})])[0]
        lines = [line for line in contexts[contract].splitlines()
                 if f'{HOST}/{repo}' in line and re.search(re.escape(site.split('::')[2]) + r'(?![\w.\[])', line)]
        # A provider that declares nothing may be shown so on the line: only the consumer's own words are keyed.
        shown = (len(lines) == 1 and (PMISMATCH in lines[0]) == bool(flags)
                 and (NOT_DECLARED in lines[0] if block == NOT_DECLARED else case == 'perm_provider_not_declared' or NOT_DECLARED not in lines[0])
                 and (not isinstance(block, dict) or all(name in lines[0] and level in lines[0] for name, level in block.items())))
        run.check(f'{case}: `context` on its provider shows its declared permissions and {"the flag" if flags else "no flag"}', shown, lines or contexts[contract])
    # Only a link whose consumer is a CI job compares permissions: an action declares none.
    site = located('perm_action_step')[0]
    rows = lambda section: gathered(exports, 'permissions', section)
    found = [row for row in rows('consumers') if row['site'] == site and (row['contract'] or '').startswith('ci::')]
    reasons = [row['reason'] for row in rows('links') if row['consumer'] == site]
    run.check('perm_action_step: a call to an action carries no permissions',
              len(found) == 1 and PERMISSIONS not in found[0] and len(reasons) == 1 and PMISMATCH not in reasons[0] and NOT_DECLARED not in reasons[0], [found, reasons])
    said = [reply for side, _, _, answers in run.asked if side == 'head' for reply in answers if 'granted' in reply.lower()]
    run.check('no answer says a permission is granted', not said, said)


def corpus(run):
    """The pinned public corpus: each repository alone against the base, then the three as one workspace."""
    roots = {}
    for side in run.bins:
        for name, repository in CORPUS.items():
            roots[side, name] = run.folder / side / 'work' / name
            if side == 'head':
                fetch(name, roots[side, name])
            else:
                shutil.copytree(roots['head', name], roots[side, name], symlinks=True)
    exports = {}
    for side in run.bins:  # alone first: an index written by a workspace run would not be a repository's own
        for name in CORPUS:
            exports[side, name] = json.loads(run.command(side, roots[side, name], f'{name}-export', 'export'))
            run.command(side, roots[side, name], f'{name}-status', 'status')
    for name, repository in CORPUS.items():
        tracked = git('ls-files', '-z', '--', W, '*action.yml', '*action.yaml', cwd=roots['head', name]).split('\0')
        files = set(owned_files({file: (roots['head', name] / file).read_text(errors='replace') for file in tracked if file}))
        parity(run, f'corpus {name}', exports['base', name], exports['head', name], files)
        local = [row for row in exports['head', name].get('links', []) if row['contract'].startswith('ci::')]
        # `ci-slsa` calls its own pipelines by local path: alone, it holds links.
        run.check(f'corpus {name}: alone, every CI link stays inside the repository',
                  all(row['provider'].startswith(repository + '::') for row in local) and (local or name != 'ci-slsa'), len(local))
    folder = run.folder / 'workspace'
    write(folder / 'workspace.toml', 'index_dir = "indexes"\n' + ''.join(
        f'\n[[repository]]\nrepo = "{repository}"\npath = {json.dumps(str(roots["head", name]))}\n' for name, repository in CORPUS.items()))
    run.command('head', folder, 'workspace-index', 'index', '--workspace', 'workspace.toml')
    links = []
    for name, repository in CORPUS.items():
        export = json.loads(run.command('head', folder, f'workspace-{name}-export', 'export', '--workspace', 'workspace.toml', '--repo', repository))
        links += [row for row in export.get('links', []) if row['contract'].startswith('ci::') and row not in links]
    # Every link goes to the fresh checker of #835, who reads each against the source; these four are read here.
    dump(folder / 'ci-links.json', {'levels': {level: sum(row['level'] == level for row in links) for level in sorted({row['level'] for row in links})}, 'links': links})
    slsa, scanner, osv = CORPUS.values()
    sha = {name: git('rev-parse', 'HEAD', cwd=roots['head', name]).strip() for name in CORPUS}
    keys = [  # consumer, contract, the Git ref it wrote, the repository whose indexed commit the reason names, `ref mismatch`
        (f'{scanner}::{W}goreleaser.yml::jobs.provenance', f'ci::{slsa}/{W}generator_generic_slsa3.yml', 'v2.1.0', 'ci-slsa', False),
        (f'{scanner}::{W}prerelease-check.yml::jobs["osv-scan"]', f'ci::{osv}/{W}osv-scanner-reusable.yml', 'main', 'ci-osv-action', False),
        (f'{slsa}::{W}pre-submit.e2e.maven.yml::jobs.build', f'ci::{slsa}/{W}builder_maven_slsa3.yml', 'main', 'ci-slsa', False),
        (f'{slsa}::{W}builder_maven_slsa3.yml::jobs["slsa-run"]', f'ci::{slsa}/{W}delegator_lowperms-generic_slsa3.yml', 'main', 'ci-slsa', False),
        (f'{osv}::{W}osv-scanner-unified-workflow.yml::jobs["scan-scheduled"]', f'ci::{osv}/{W}osv-scanner-reusable.yml',
         '87a33814e525266e92b7ecaa23a9743ee5a702b8', 'ci-osv-action', True),
    ]
    for consumer, contract, ref, provider, differs in keys:
        found = [row for row in links if row['consumer'] == consumer and row['contract'] == contract]
        shape = flagged(found[0]['reason']) if len(found) == 1 else None
        run.check(f'corpus: {consumer} calls {contract} at @{ref}', bool(shape) and found[0]['level'] == 'exact' and shape[0] == ref
                  and sha[provider].startswith(shape[1]) and ('ref mismatch' in shape[2]) == differs, found)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--corpus', action='store_true', help='drive the pinned public corpus instead of the generated workspaces')
    parser.add_argument('--slice', choices=sorted(SLICES), default='f1', help='the slice of #820 whose workspaces are driven')
    parser.add_argument('--head-bin')
    parser.add_argument('--base-bin')
    parser.add_argument('--run')
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    if args.selftest:
        selftest()
        return 0
    if not args.head_bin or not args.base_bin or not args.run:
        parser.error('--head-bin, --base-bin and --run are required: half of the answer keys are the base binary')
    folder = Path(args.run).resolve()
    folder.mkdir(parents=True, exist_ok=False)
    run = Run(folder, str(Path(args.head_bin).resolve()), str(Path(args.base_bin).resolve()))
    corpus(run) if args.corpus else drive(run, args.http, args.slice)
    failed = [row['name'] for row in run.checks if not row['passed']]
    title = 'the pinned corpus' if args.corpus else f'slice {args.slice.upper()}'
    write(folder / 'receipt.md', f'# CI in the graph, {title} (#820)\n\nAnswer key: #820 and the generated files, in `input/`.\n\n'
          + ('Checks passed; independent verdict pending.\n' if not failed else 'BROKEN checks:\n' + ''.join(f'- {name}\n' for name in failed)))
    return bool(failed)


if __name__ == '__main__':
    sys.exit(main())
