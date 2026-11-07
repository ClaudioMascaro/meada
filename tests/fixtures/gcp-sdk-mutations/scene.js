import {SecretManagerServiceClient} from '@google-cloud/secret-manager';
export function Unchanged() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/steady-token/versions/latest'};client.accessSecretVersion(request); }
export function DirectWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-direct/versions/latest'};request.name='projects/review/secrets/current-token/versions/latest';client.accessSecretVersion(request); }
export function AliasWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-alias/versions/latest'};const alias=request;alias.name='projects/review/secrets/current-token/versions/latest';client.accessSecretVersion(request); }
export function BeforeWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/before-write/versions/latest'};client.accessSecretVersion(request);request.name='after'; }
export function OtherObject() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/other-object/versions/latest'};const other={name:'x'};other.name='y';client.accessSecretVersion(request); }
export function AliasSelected() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-selected/versions/latest'};const alias=request;request.name='after';client.accessSecretVersion(alias); }
function mutate(request) { request.name='after'; }
export function HelperWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-helper/versions/latest'};mutate(request);client.accessSecretVersion(request); }
export function LateHelperWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/late-helper/versions/latest'};client.accessSecretVersion(request);mutate(request); }
export function MappingWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-update/versions/latest'};Object.assign(request,{name:'after'});client.accessSecretVersion(request); }
export function DeleteWrite() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-delete/versions/latest'};delete request.name;client.accessSecretVersion(request); }
export function OtherField() { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/other-field/versions/latest'};request.timeout=1;client.accessSecretVersion(request); }
export function OpaqueAlias(flag) { const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-opaque/versions/latest'};const other={name:'x'};let alias=request;if(flag) alias=other;alias.name='after';client.accessSecretVersion(request); }
export function PatternWrite() {const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-pattern/versions/latest'};([request.name]=['after']);client.accessSecretVersion(request);}

function mutateOpaque(request, flag) {let alias=request;if(flag)alias={name:'other'};alias.name='after';}
export function HelperOpaque(flag) {const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/stale-helper-opaque/versions/latest'};mutateOpaque(request,flag);client.accessSecretVersion(request);}
export function LateHelperOpaque(flag) {const client=new SecretManagerServiceClient();const request={name:'projects/review/secrets/late-helper-opaque/versions/latest'};client.accessSecretVersion(request);mutateOpaque(request,flag);}
