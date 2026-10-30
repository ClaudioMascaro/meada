{{- /* The [catalog] and [workspaces] sections of named workspaces (ADR 0021): the console's and the refresh job's bootstrap carry them, a node's never does. */ -}}
{{- define "meada.namedWorkspaces" -}}
{{- with .Values.modules.workspaces }}
{{- if .enabled }}

[catalog]
allow = {{ .allow | toJson }}
max_repositories = {{ .maxRepositories }}

[workspaces]
mirrors = {{ .mirrors | toJson }}
max_workspaces = {{ .maxWorkspaces }}
max_entries = {{ .maxEntries }}
{{- end }}
{{- end }}
{{- end -}}
{{- /* [workspaces] needs [ui] with operations in every process that reads it, the refresh job included. */ -}}
{{- define "meada.jobUi" -}}
{{- if .Values.modules.workspaces.enabled }}

[ui]
dir = "/usr/share/meada/ui"
public_url = {{ .Values.console.publicUrl | toJson }}
client_id = {{ .Values.console.clientId | toJson }}
audience = {{ .Values.console.audience | toJson }}
operations = {{ .Values.console.operations }}
{{- with .Values.console.operatorGroups }}
operator_groups = {{ . | toJson }}
{{- end }}
{{- with .Values.console.operatorClaim }}
operator_claim = {{ . | toJson }}
{{- end }}
{{- end }}
{{- end -}}
