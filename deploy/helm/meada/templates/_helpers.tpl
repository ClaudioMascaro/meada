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
{{- if .Values.console.enabled -}}
{{- if not (and .Values.image.cloud .Values.modules.snapshot.enabled .Values.modules.generations.enabled .Values.modules.valkey.enabled) -}}
{{- fail "console requires image.cloud and snapshot, valkey and generations" -}}
{{- end -}}
{{- if and (or .Values.console.operations .Values.console.operatorGroups .Values.console.operatorClaim) (not .Values.modules.oidc.enabled) -}}
{{- fail "console operations and operator selectors require modules.oidc" -}}
{{- end -}}
{{- if and .Values.console.operatorGroups .Values.console.operatorClaim -}}
{{- fail "choose console.operatorGroups or console.operatorClaim" -}}
{{- end -}}
{{- if and .Values.console.operations (not (or .Values.console.operatorGroups .Values.console.operatorClaim)) -}}
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
{{- if .Values.modules.oidc.github.enabled }}meada github-app roles --workspace {{ $ws }} && meada workspace show --pulled --workspace {{ $ws }} > /data/pulled.toml && /bin/sh /etc/meada/fetch.sh /data/pulled.toml{{ else }}/bin/sh /etc/meada/fetch.sh{{ end }} && meada index --workspace {{ $ws }} && meada snapshot publish --workspace {{ $ws }}
{{- end -}}
