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
