{{- define "oxabl-cache.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oxabl-cache.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "oxabl-cache.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "oxabl-cache.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "oxabl-cache.selectorLabels" -}}
app.kubernetes.io/name: {{ include "oxabl-cache.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "oxabl-cache.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "oxabl-cache.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{- define "oxabl-cache.image" -}}
{{- if .Values.image.digest -}}
{{- printf "%s@%s" .Values.image.repository .Values.image.digest -}}
{{- else -}}
{{- printf "%s:%s" .Values.image.repository (default .Chart.AppVersion .Values.image.tag) -}}
{{- end -}}
{{- end -}}

{{- define "oxabl-cache.singleNode" -}}
{{- eq .Values.mode "single-node" -}}
{{- end -}}

{{- define "oxabl-cache.replicas" -}}
{{- if eq .Values.mode "single-node" -}}1{{- else -}}{{- .Values.replicaCount -}}{{- end -}}
{{- end -}}

{{/* Fails the render when a value the chosen mode needs is missing. */}}
{{- define "oxabl-cache.validate" -}}
{{- $v := .Values -}}
{{- if not (has $v.mode (list "cluster" "single-node")) -}}{{- fail "mode is cluster or single-node" -}}{{- end -}}
{{- if not $v.publicUrl -}}{{- fail "publicUrl is required (the URL clients reach the public listener on)" -}}{{- end -}}
{{- if not $v.keys.existingSecret -}}{{- fail "keys.existingSecret is required: a Secret with url-key and claim-key" -}}{{- end -}}
{{- if not (has $v.tls.mode (list "upstream" "secret")) -}}{{- fail "tls.mode is upstream or secret" -}}{{- end -}}
{{- if and (eq $v.tls.mode "secret") (not $v.tls.existingSecret) -}}{{- fail "tls.existingSecret is required when tls.mode is secret" -}}{{- end -}}
{{- if eq $v.mode "cluster" -}}
{{- if not $v.metadata.postgres.url -}}{{- fail "metadata.postgres.url is required in cluster mode" -}}{{- end -}}
{{- if not $v.metadata.postgres.passwordSecret.name -}}{{- fail "metadata.postgres.passwordSecret.name is required in cluster mode" -}}{{- end -}}
{{- with $v.store.s3 -}}
{{- if not (and .endpoint .region .bucket .credentialsSecret.name) -}}{{- fail "store.s3 needs endpoint, region, bucket and credentialsSecret.name in cluster mode" -}}{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}
