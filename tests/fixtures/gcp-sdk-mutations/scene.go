package review
import (
 secretmanager "cloud.google.com/go/secretmanager/apiv1"
 secretpb "cloud.google.com/go/secretmanager/apiv1/secretmanagerpb"
)
func Unchanged() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/steady-token/versions/latest"};client.AccessSecretVersion(ctx,request) }
func DirectWrite() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/stale-direct/versions/latest"};request.Name="projects/review/secrets/current-token/versions/latest";client.AccessSecretVersion(ctx,request) }
func AliasWrite() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/stale-alias/versions/latest"};alias:=request;alias.Name="projects/review/secrets/current-token/versions/latest";client.AccessSecretVersion(ctx,request) }
func BeforeWrite() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/before-write/versions/latest"};client.AccessSecretVersion(ctx,request);request.Name="after" }
func OtherObject() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/other-object/versions/latest"};other:=&secretpb.AccessSecretVersionRequest{Name:"x"};other.Name="y";client.AccessSecretVersion(ctx,request) }
func AliasSelected() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/stale-selected/versions/latest"};alias:=request;request.Name="after";client.AccessSecretVersion(ctx,alias) }
func mutate(request *secretpb.AccessSecretVersionRequest) { request.Name="after" }
func HelperWrite() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/stale-helper/versions/latest"};mutate(request);client.AccessSecretVersion(ctx,request) }
func LateHelperWrite() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/late-helper/versions/latest"};client.AccessSecretVersion(ctx,request);mutate(request) }
func OtherField() { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/other-field/versions/latest"};request.Timeout=1;client.AccessSecretVersion(ctx,request) }
func OpaqueAlias(flag bool) { client,err:=secretmanager.NewClient(ctx);request:=&secretpb.AccessSecretVersionRequest{Name:"projects/review/secrets/stale-opaque/versions/latest"};other:=&secretpb.AccessSecretVersionRequest{Name:"x"};alias:=request;if flag { alias=other };alias.Name="after";client.AccessSecretVersion(ctx,request) }
