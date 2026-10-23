# Meada en Kubernetes

[Português](README.md) · [English](README.en.md)

Configure la admisión en el Ingress o proxy de autenticación antes de exponer el servicio.
Sin `[oidc]`, ese componente admite clientes; una configuración incorrecta expone el grafo
de código. Con OIDC, preserve `Authorization`. Preserve también los headers SEP-2243
`MCP-Protocol-Version`, `MCP-Session-Id`, `Mcp-Method`, `Mcp-Name` y `Mcp-Param-*`,
devuelva `Meada-Generation` y nunca añada `Origin`.
El chart no fija annotations de un controller: configure la autenticación de su Ingress.

## Instalación

Las releases publican el chart en `oci://ghcr.io/claudiomascaro/charts/meada`. Use la misma
versión del chart y de Meada:

```sh
helm upgrade --install meada oci://ghcr.io/claudiomascaro/charts/meada --version <versión> -f mis-valores.yaml
```

Desde el checkout, sustituya la referencia OCI por `deploy/helm/meada`. Los valores por defecto
crean dos nodos sin módulos cloud. Cada pod obtiene sus propios mirrors e indexa en su propio
`emptyDir` mediante un init container antes de iniciar el servidor. Ajuste
`workspace.repositories` (TOML) y `workspace.fetch` (script ejecutado fuera de Meada).
`workspace.gitCredentialsSecret` aporta variables de un Secret al fetch. `meada index` nunca
hace fetch ni accede a la red.

## Generaciones

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml -f mis-valores.yaml
```

Configure endpoints externos `https://` para el store y `rediss://` para Valkey. Cree los
Secrets `meada-valkey` (`password`) y `meada-store` (`access-key-id`, `secret-access-key` y,
opcionalmente, `session-token`), o configure sus nombres en `modules`. La URL de Valkey nombra
el usuario AUTH; la contraseña viene solamente del Secret. Para una CA privada, configure
`tls.caSecret` o `tls.caConfigMap`, con la clave `ca.crt`. El chart no crea esos servicios ni
guarda credenciales en manifests o valores.

Cada pod hace un pull inicial y mantiene un companion `pull --every 30` que comparte solamente
el `local_dir` de ese pod. El CronJob usa `concurrencyPolicy: Forbid`, sus propios mirrors e
index_dir, y ejecuta fetch, index y publish dentro de `meada snapshot lock`. No comparta índices
entre pods. La readiness consulta el estado local con `meada status`, o TCP con
`readiness.type=tcp`.

`modules.snapshot.credentials` acepta `static`, `web-identity` y `pod-identity`. Para IRSA,
configure la annotation de la role en `serviceAccount.annotations`; el webhook de la plataforma
inyecta `AWS_ROLE_ARN` y `AWS_WEB_IDENTITY_TOKEN_FILE`. Para Pod Identity, asocie la service
account a su role en la plataforma. No hay soporte para IAM AUTH de ElastiCache.

Habilite OIDC en `modules.oidc.enabled`, configurando issuer, publicUrl y claims/grupos del
proveedor. Los módulos deshabilitados no generan secciones TOML. `image.cloud=true` selecciona
la tag `-cloud`; `image.tag` vacío sigue `appVersion`. Una sección cloud necesita la imagen cloud.

## Operación y validación

El Service es ClusterIP sin afinidad de sesión. La gracia por defecto es 40 s, incluyendo el
preStop de 5 s y el drain de hasta 25 s. Ajuste `resources`, `nodeSelector`, `tolerations` y
`affinity`. El checksum del manifest hace rollout al cambiar el ConfigMap.

```sh
helm lint deploy/helm/meada
helm template meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml
python3 scripts/helm-check
```

`helm-check` necesita Python 3.11+ y PyYAML. CI valida los templates sin crear un cluster.
El smoke local usa un cluster kind descartable, la imagen default cargada, un repositorio de
prueba creado por el init container y `search` por port-forward.
