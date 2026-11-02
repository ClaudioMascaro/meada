{{- define "meada.name" -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- define "meada.labels" -}}
app.kubernetes.io/name: meada
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{- end -}}
{{- define "meada.serverLabels" -}}
{{ include "meada.labels" . }}
app.kubernetes.io/component: server
{{- end -}}
{{- define "meada.image" -}}
{{- $tag := default .Chart.AppVersion .Values.image.tag -}}
{{- if and .Values.image.cloud (not (hasSuffix "-cloud" $tag)) -}}
{{- $tag = printf "%s-cloud" $tag -}}
{{- end -}}
{{- printf "%s:%s" .Values.image.repository $tag -}}
{{- end -}}
{{- define "meada.account" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "meada.name" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}
{{- define "meada.args" -}}
- mcp
- --listen
- {{ printf "0.0.0.0:%v" .Values.listen | quote }}
- --workspace
- /etc/meada/meada.workspace.toml
{{- if .Values.modules.snapshot.enabled }}
- --generations
{{- end }}
{{- end -}}
{{- define "meada.validate" -}}
{{- if le (int .Values.terminationGracePeriodSeconds) (add 25 (int .Values.preStop.sleepSeconds)) -}}
{{- fail "terminationGracePeriodSeconds must exceed the 25 second drain plus preStop.sleepSeconds" -}}
{{- end -}}
{{- if and .Values.tls.caSecret .Values.tls.caConfigMap -}}
{{- fail "choose tls.caSecret or tls.caConfigMap" -}}
{{- end -}}
{{- range $name, $module := .Values.modules -}}
{{- if and $module.enabled (not $.Values.image.cloud) -}}
{{- fail (printf "modules.%s.enabled requires image.cloud" $name) -}}
{{- end -}}
{{- end -}}
{{- if and .Values.modules.snapshot.enabled (not (hasPrefix "https://" .Values.modules.snapshot.endpoint)) -}}
{{- fail "modules.snapshot.endpoint must use https" -}}
{{- end -}}
{{- if and .Values.modules.snapshot.enabled (regexMatch "^https://[^/]*@" .Values.modules.snapshot.endpoint) -}}
{{- fail "modules.snapshot.endpoint must not contain credentials; use a Secret or platform identity" -}}
{{- end -}}
{{- if and .Values.modules.valkey.enabled (not (hasPrefix "rediss://" .Values.modules.valkey.url)) -}}
{{- fail "modules.valkey.url must use rediss" -}}
{{- end -}}
{{- if and .Values.modules.valkey.enabled (regexMatch "^rediss://[^/@]*:[^/@]*@" .Values.modules.valkey.url) -}}
{{- fail "modules.valkey.url must not contain a password; use credentialsSecret" -}}
{{- end -}}
{{- if and .Values.modules.generations.enabled (not (and .Values.modules.snapshot.enabled .Values.modules.valkey.enabled)) -}}
{{- fail "modules.generations requires snapshot and valkey" -}}
{{- end -}}
{{- if and .Values.modules.refreshLock.enabled (not .Values.modules.generations.enabled) -}}
{{- fail "modules.refreshLock requires generations" -}}
{{- end -}}
{{- if not (has .Values.workspace.source (list "file" "store")) -}}
{{- fail "workspace.source must be file or store" -}}
{{- end -}}
{{- if and (eq .Values.workspace.source "store") (not .Values.modules.refreshLock.enabled) -}}
{{- fail "workspace.source=store requires modules.refreshLock for the managed refresh" -}}
{{- end -}}
{{- if not (has .Values.modules.oidc.type (list "generic" "entra" "okta" "keycloak")) -}}
{{- fail "modules.oidc.type must be generic, entra, okta or keycloak" -}}
{{- end -}}
{{- if and (eq .Values.modules.oidc.entra.overage "directory") (not (and (eq .Values.modules.oidc.type "entra") .Values.modules.directory.enabled .Values.modules.oidc.groupsClaim)) -}}
{{- fail "modules.oidc.entra.overage=directory requires type entra, modules.directory and modules.oidc.groupsClaim" -}}
{{- end -}}
{{- if and .Values.modules.oidc.github.enabled (not (and .Values.modules.oidc.enabled .Values.modules.postgres.enabled .Values.modules.githubApp.enabled .Values.modules.oidc.github.loginClaim)) -}}
{{- fail "modules.oidc.github requires modules.oidc, modules.postgres, modules.githubApp and loginClaim" -}}
{{- end -}}
{{- if and .Values.modules.directory.enabled (not (and .Values.modules.oidc.enabled .Values.modules.postgres.enabled .Values.modules.directory.userClaim)) -}}
{{- fail "modules.directory requires modules.oidc, modules.postgres and userClaim" -}}
{{- end -}}
{{- if and .Values.postgres.enabled (not .Values.modules.postgres.enabled) -}}
{{- fail "postgres.enabled requires modules.postgres" -}}
{{- end -}}
{{- with .Values.modules.accounts -}}
{{- if .enabled -}}
{{- if not $.Values.modules.postgres.enabled -}}
{{- fail "modules.accounts requires modules.postgres" -}}
{{- end -}}
{{- if not $.Values.console.enabled -}}
{{- fail "modules.accounts requires console.enabled: the console holds the accounts and signs the tokens" -}}
{{- end -}}
{{- if not (and .publicKeySecret .privateKeySecret) -}}
{{- fail "modules.accounts requires publicKeySecret and privateKeySecret" -}}
{{- end -}}
{{- if eq .publicKeySecret .privateKeySecret -}}
{{- fail "modules.accounts.publicKeySecret and privateKeySecret must be different Secrets: the nodes mount the public one" -}}
{{- end -}}
{{- if and $.Values.modules.oidc.enabled (not .breakGlass) -}}
{{- fail "modules.accounts beside modules.oidc requires modules.accounts.breakGlass" -}}
{{- end -}}
{{- if and .breakGlass (not $.Values.modules.oidc.enabled) -}}
{{- fail "modules.accounts.breakGlass requires modules.oidc" -}}
{{- end -}}
{{- if and .breakGlass (or .groups .github.enabled) -}}
{{- fail "modules.accounts.groups and modules.accounts.github are refused under breakGlass: the break-glass account is the global operator" -}}
{{- end -}}
{{- if and .github.enabled (not (and $.Values.modules.githubApp.enabled .github.host)) -}}
{{- fail "modules.accounts.github requires modules.githubApp and a host" -}}
{{- end -}}
{{- if and .github.enabled (ne .github.host "github.com") (not .github.api) -}}
{{- fail "modules.accounts.github.api is required for a host other than github.com" -}}
{{- end -}}
{{- else if or .breakGlass .groups .github.enabled -}}
{{- fail "modules.accounts.breakGlass, groups and github require modules.accounts.enabled" -}}
{{- end -}}
{{- end -}}
{{- if .Values.modules.workspaces.enabled -}}
{{- if not (and .Values.console.enabled .Values.console.operations (or .Values.modules.oidc.github.enabled .Values.modules.accounts.github.enabled)) -}}
{{- fail "modules.workspaces requires console.enabled, console.operations and modules.oidc.github or modules.accounts.github" -}}
{{- end -}}
{{- if not .Values.modules.workspaces.allow -}}
{{- fail "modules.workspaces.allow must list at least one pattern" -}}
{{- end -}}
{{- $max := int .Values.modules.workspaces.maxRepositories -}}
{{- if or (lt $max 1) (gt $max 5000) -}}
{{- fail "modules.workspaces.maxRepositories must be set (1 to 5000): it has no default" -}}
{{- end -}}
{{- end -}}
{{- if .Values.proposals.enabled -}}
{{- if not .Values.modules.workspaces.enabled -}}
{{- fail "proposals.enabled requires modules.workspaces: the proposal server reads [workspaces] and [catalog]" -}}
{{- end -}}
{{- /* The binary's bare URL: https, a host, no user, path, query or fragment. */ -}}
{{- if not (regexMatch "^https://[^/?#@]+/?$" .Values.proposals.publicUrl) -}}
{{- fail "proposals.publicUrl must be a bare https URL (no path, query, fragment or user)" -}}
{{- end -}}
{{- end -}}
{{- if .Values.console.enabled -}}
{{- if not (and .Values.image.cloud .Values.modules.snapshot.enabled .Values.modules.generations.enabled .Values.modules.valkey.enabled) -}}
{{- fail "console requires image.cloud and snapshot, valkey and generations" -}}
{{- end -}}
{{- if and (or .Values.console.operatorGroups .Values.console.operatorClaim) (not .Values.modules.oidc.enabled) -}}
{{- fail "console operator selectors require modules.oidc" -}}
{{- end -}}
{{- if and .Values.console.operations (not (or .Values.modules.oidc.enabled .Values.modules.accounts.enabled)) -}}
{{- fail "console.operations requires modules.oidc or modules.accounts" -}}
{{- end -}}
{{- if and .Values.console.operatorGroups .Values.console.operatorClaim -}}
{{- fail "choose console.operatorGroups or console.operatorClaim" -}}
{{- end -}}
{{- if and .Values.console.operations .Values.modules.oidc.enabled (not (or .Values.console.operatorGroups .Values.console.operatorClaim)) -}}
{{- fail "console.operations requires an operator selector" -}}
{{- end -}}
{{- if and .Values.console.operatorGroups (not .Values.modules.oidc.groupsClaim) -}}
{{- fail "console.operatorGroups requires modules.oidc.groupsClaim" -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- define "meada.mounts" -}}
- name: config
  mountPath: /etc/meada
  readOnly: true
- name: data
  mountPath: /data
{{- if or .Values.tls.caSecret .Values.tls.caConfigMap }}
- name: ca
  mountPath: /etc/meada-ca
  readOnly: true
{{- end }}
{{- end -}}
{{- define "meada.volumes" -}}
- name: config
  configMap:
    name: {{ include "meada.name" . }}
- name: data
  emptyDir: {}
{{- if .Values.tls.caSecret }}
- name: ca
  secret:
    secretName: {{ .Values.tls.caSecret }}
{{- else if .Values.tls.caConfigMap }}
- name: ca
  configMap:
    name: {{ .Values.tls.caConfigMap }}
{{- end }}
{{- end -}}
{{- define "meada.env" -}}
{{- if .Values.modules.valkey.enabled }}
- name: MEADA_VALKEY_PASSWORD
  valueFrom:
    secretKeyRef:
      name: {{ .Values.modules.valkey.credentialsSecret }}
      key: password
{{- end }}
{{- if and .Values.modules.snapshot.enabled (eq .Values.modules.snapshot.credentials "static") }}
- name: MEADA_STORE_KEY
  valueFrom:
    secretKeyRef:
      name: {{ .Values.modules.snapshot.credentialsSecret }}
      key: access-key-id
- name: MEADA_STORE_SECRET
  valueFrom:
    secretKeyRef:
      name: {{ .Values.modules.snapshot.credentialsSecret }}
      key: secret-access-key
- name: MEADA_STORE_TOKEN
  valueFrom:
    secretKeyRef:
      name: {{ .Values.modules.snapshot.credentialsSecret }}
      key: session-token
      optional: true
{{- end }}
{{- end -}}
{{- define "meada.consoleName" -}}
{{- printf "%s-console" (include "meada.name" . | trunc 55 | trimSuffix "-") -}}
{{- end -}}
{{- define "meada.consoleLabels" -}}
{{ include "meada.labels" . }}
app.kubernetes.io/component: console
{{- end -}}
{{- define "meada.consoleVolumes" -}}
- name: config
  configMap:
    name: {{ include "meada.consoleName" . }}
- name: data
  emptyDir: {}
{{- if .Values.modules.accounts.enabled }}
- name: accounts-public
  secret:
    secretName: {{ .Values.modules.accounts.publicKeySecret }}
- name: accounts-private
  secret:
    secretName: {{ .Values.modules.accounts.privateKeySecret }}
{{- end }}
{{- if .Values.tls.caSecret }}
- name: ca
  secret:
    secretName: {{ .Values.tls.caSecret }}
{{- else if .Values.tls.caConfigMap }}
- name: ca
  configMap:
    name: {{ .Values.tls.caConfigMap }}
{{- end }}
{{- end -}}
{{- define "meada.proposalsName" -}}
{{- printf "%s-proposals" (include "meada.name" . | trunc 53 | trimSuffix "-") -}}
{{- end -}}
{{- define "meada.proposalsLabels" -}}
{{ include "meada.labels" . }}
app.kubernetes.io/component: proposals
{{- end -}}
{{- /* The proposal server holds the public half of the accounts key, never the private one (ADR 0022, ADR 0024). */ -}}
{{- define "meada.proposalsVolumes" -}}
- name: config
  configMap:
    name: {{ include "meada.proposalsName" . }}
- name: data
  emptyDir: {}
{{- include "meada.accountsPublicVolume" . }}
{{- if .Values.tls.caSecret }}
- name: ca
  secret:
    secretName: {{ .Values.tls.caSecret }}
{{- else if .Values.tls.caConfigMap }}
- name: ca
  configMap:
    name: {{ .Values.tls.caConfigMap }}
{{- end }}
{{- end -}}
{{- /* [proposals] (ADR 0022): the proposal server's bootstrap reads it, and the console's reads it to issue agent tokens for the surface under local accounts. */ -}}
{{- define "meada.proposalsSection" -}}
{{- with .Values.proposals }}
{{- if .enabled }}

[proposals]
public_url = {{ .publicUrl | toJson }}
{{- with .audience }}
audience = {{ . | toJson }}
{{- end }}
{{- end }}
{{- end }}
{{- end -}}
{{- define "meada.postgresName" -}}
{{- printf "%s-postgres" (include "meada.name" . | trunc 54 | trimSuffix "-") -}}
{{- end -}}
{{- /* MEADA_POSTGRES_URL from the Secret's key: write-url for the console and the refresh, read-url for the nodes. */ -}}
{{- define "meada.postgresEnv" -}}
{{- if .root.Values.modules.postgres.enabled }}
- name: MEADA_POSTGRES_URL
  valueFrom:
    secretKeyRef:
      name: {{ .root.Values.modules.postgres.credentialsSecret }}
      key: {{ .key }}
{{- end }}
{{- end -}}
{{- /* The init container that holds the console and the refresh back until the schema is migrated: `meada status` reports "postgres: reachable" only at a schema version it knows. Nodes start and serve without Postgres (#332), so they never wait. */ -}}
{{- define "meada.waitPostgres" -}}
{{- if .root.Values.modules.postgres.enabled }}
- name: wait-postgres
  image: {{ include "meada.image" .root }}
  imagePullPolicy: {{ .root.Values.image.pullPolicy }}
  command: ["/bin/sh", "-c"]
  args:
    - until meada status --workspace /etc/meada/postgres.workspace.toml 2>/dev/null | grep -q '^postgres: reachable'; do sleep 3; done
  env:
    - name: LC_ALL
      value: C
    {{- include "meada.postgresEnv" . | nindent 4 }}
  volumeMounts: {{ include "meada.mounts" .root | nindent 4 }}
{{- end }}
{{- end -}}
{{- /* The refresh job's workspace bootstrap: with modules.githubApp it carries the App's section, which the nodes' and console's never do. */ -}}
{{- define "meada.jobWorkspace" -}}
{{- if .Values.modules.githubApp.enabled -}}/etc/meada/job.workspace.toml{{- else -}}/etc/meada/meada.workspace.toml{{- end -}}
{{- end -}}
{{- define "meada.refresh" -}}
{{- $ws := include "meada.jobWorkspace" . -}}
{{- if eq .Values.workspace.source "store" }}meada workspace pull --workspace {{ $ws }} && {{ end -}}
{{- if or .Values.modules.oidc.github.enabled .Values.modules.accounts.github.enabled }}meada github-app roles --workspace {{ $ws }} && meada workspace show --pulled --workspace {{ $ws }} > /data/pulled.toml && /bin/sh /etc/meada/fetch.sh /data/pulled.toml{{ else }}/bin/sh /etc/meada/fetch.sh{{ end }} && meada index --workspace {{ $ws }} && meada snapshot publish --workspace {{ $ws }}
{{- end -}}
{{- /* [accounts] and its tables (ADR 0024) for one process: "console" names both key files and the break-glass flag, "node" the public key and the grant tables (a node ignores [accounts] beside [oidc], so under breakGlass it gets none), "job" only the GitHub grant provider the role check reads, with no key. */ -}}
{{- define "meada.accounts" -}}
{{- $role := .role -}}
{{- with .Values.modules.accounts }}
{{- if and .enabled (or (eq $role "console") (not .breakGlass)) (or (ne $role "job") .github.enabled) }}

[accounts]
{{- if ne $role "job" }}
public_key_file = "/etc/meada-accounts/public/public-key"
{{- end }}
{{- if eq $role "console" }}
private_key_file = "/etc/meada-accounts/private/private-key"
{{- with .adminPasswordSecret }}
admin_password_env = "MEADA_ADMIN_PASSWORD"
{{- end }}
{{- if .breakGlass }}
break_glass = true
{{- end }}
{{- end }}
{{- if and .groups (ne $role "job") }}

[accounts.groups]
{{- range $group, $identities := .groups }}
{{ $group | toJson }} = {{ $identities | toJson }}
{{- end }}
{{- end }}
{{- with .github }}
{{- if .enabled }}

[accounts.github]
host = {{ .host | toJson }}
{{- with .api }}
api = {{ . | toJson }}
{{- end }}
{{- if .roles }}
roles = {{ .roles | toJson }}
{{- end }}
max_age_seconds = {{ .maxAgeSeconds }}
{{- end }}
{{- end }}
{{- end }}
{{- end }}
{{- end -}}
{{- /* A node's [ui] under local accounts: only the console's public_url, the issuer of every token it admits (ADR 0024). */ -}}
{{- define "meada.nodeUi" -}}
{{- if and .Values.modules.accounts.enabled (not .Values.modules.accounts.breakGlass) }}

[ui]
dir = "/usr/share/meada/ui"
public_url = {{ .Values.console.publicUrl | toJson }}
{{- end }}
{{- end -}}
{{- /* The Secrets of the keys as volumes: the public one in the console and the nodes, the private one in the console only. */ -}}
{{- define "meada.accountsPublicVolume" -}}
{{- if and .Values.modules.accounts.enabled (not .Values.modules.accounts.breakGlass) }}
- name: accounts-public
  secret:
    secretName: {{ .Values.modules.accounts.publicKeySecret }}
{{- end }}
{{- end -}}
{{- define "meada.accountsPublicMount" -}}
- name: accounts-public
  mountPath: /etc/meada-accounts/public
  readOnly: true
{{- end -}}

{{/* The [postgres] block of a workspace manifest: dict "urlEnv" "schema" "ca" (true when ca_file is set). */}}
{{- define "meada.postgresBlock" -}}
[postgres]
url_env = {{ .urlEnv | toJson }}
schema = {{ .schema | toJson }}
{{- if .ca }}
ca_file = "/etc/meada-ca/ca.crt"
{{- end }}
{{- end }}
