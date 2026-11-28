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
