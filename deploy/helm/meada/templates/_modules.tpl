{{- define "meada.modules" -}}
{{- with .Values.modules.snapshot }}
{{- if .enabled }}

[snapshot]
endpoint = {{ .endpoint | toJson }}
region = {{ .region | toJson }}
bucket = {{ .bucket | toJson }}
prefix = {{ .prefix | toJson }}
path_style = {{ .pathStyle }}
local_dir = {{ $.localDir | toJson }}
credentials = {{ .credentials | toJson }}
{{- if or $.Values.tls.caSecret $.Values.tls.caConfigMap }}
ca_file = "/etc/meada-ca/ca.crt"
{{- end }}
{{- if eq .credentials "static" }}
access_key_id_env = "MEADA_STORE_KEY"
secret_access_key_env = "MEADA_STORE_SECRET"
session_token_env = "MEADA_STORE_TOKEN"
{{- end }}
{{- end }}
{{- end }}
{{- with .Values.modules.valkey }}
{{- if .enabled }}

[valkey]
url = {{ .url | toJson }}
key_prefix = {{ .keyPrefix | toJson }}
cluster = {{ .cluster }}
password_env = "MEADA_VALKEY_PASSWORD"
{{- if or $.Values.tls.caSecret $.Values.tls.caConfigMap }}
ca_file = "/etc/meada-ca/ca.crt"
{{- end }}
{{- end }}
{{- end }}
{{- with .Values.modules.postgres }}
{{- if .enabled }}

[postgres]
url_env = "MEADA_POSTGRES_URL"
schema = {{ .schema | toJson }}
{{- if or $.Values.tls.caSecret $.Values.tls.caConfigMap }}
ca_file = "/etc/meada-ca/ca.crt"
{{- end }}
{{- end }}
{{- end }}
{{- if .Values.modules.generations.enabled }}

[generations]
keep = {{ .Values.modules.generations.keep }}
{{- end }}
{{- if .Values.modules.refreshLock.enabled }}

[refresh_lock]
ttl_seconds = {{ .Values.modules.refreshLock.ttlSeconds }}
{{- end }}
{{- with .Values.modules.oidc }}
{{- if .enabled }}

[oidc]
{{- if ne .type "generic" }}
type = {{ .type | toJson }}
{{- end }}
issuer = {{ .issuer | toJson }}
public_url = {{ .publicUrl | toJson }}
{{- with .audience }}
audience = {{ . | toJson }}
{{- end }}
{{- with .identitiesClaim }}
identities_claim = {{ . | toJson }}
{{- end }}
{{- if .groupsClaim }}
groups_claim = {{ .groupsClaim | toJson }}
{{- end }}
scopes = {{ .scopes | toJson }}
{{- if .groupsClaim }}
[oidc.groups]
{{- range $group, $identities := .groups }}
{{ $group | toJson }} = {{ $identities | toJson }}
{{- end }}
{{- end }}
{{- if eq .type "entra" }}

[oidc.entra]
overage = {{ .entra.overage | toJson }}
{{- end }}
{{- with .github }}
{{- if .enabled }}

[oidc.github]
host = {{ .host | toJson }}
login_claim = {{ .loginClaim | toJson }}
{{- if .roles }}
roles = {{ .roles | toJson }}
{{- end }}
max_age_seconds = {{ .maxAgeSeconds }}
{{- end }}
{{- end }}
{{- end }}
{{- end }}
{{- with .Values.modules.directory }}
{{- if .enabled }}

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = {{ .userClaim | toJson }}
user_attribute = {{ .userAttribute | toJson }}
group_attribute = {{ .groupAttribute | toJson }}
{{- end }}
{{- end }}
{{- end -}}
{{- define "meada.nodeBootstrap" -}}
index_dir = "/data/indexes"
{{- if eq .Values.workspace.source "store" }}

[workspace]
source = "store"
{{- else }}
{{- .Values.workspace.repositories | nindent 0 }}
{{- end }}
{{- include "meada.modules" (dict "Values" .Values "localDir" "/data/generations") }}
{{- end -}}
