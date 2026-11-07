from google.cloud import secretmanager

def Unchanged():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/steady-token/versions/latest'}
    client.access_secret_version(request=request)
def DirectWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-direct/versions/latest'}
    request['name']='projects/review/secrets/current-token/versions/latest'
    client.access_secret_version(request=request)
def AliasWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-alias/versions/latest'}
    alias=request
    alias['name']='projects/review/secrets/current-token/versions/latest'
    client.access_secret_version(request=request)
def BeforeWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/before-write/versions/latest'}
    client.access_secret_version(request=request)
    request['name']='after'
def OtherObject():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/other-object/versions/latest'}
    other={'name':'x'}
    other['name']='y'
    client.access_secret_version(request=request)
def AliasSelected():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-selected/versions/latest'}
    alias=request
    request['name']='after'
    client.access_secret_version(request=alias)
def mutate(request):
    request['name']='after'
def HelperWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-helper/versions/latest'}
    mutate(request)
    client.access_secret_version(request=request)
def LateHelperWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/late-helper/versions/latest'}
    client.access_secret_version(request=request)
    mutate(request)
def MappingWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-update/versions/latest'}
    request.update({'name':'after'})
    client.access_secret_version(request=request)
def DeleteWrite():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-delete/versions/latest'}
    del request['name']
    client.access_secret_version(request=request)
def OtherField():
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/other-field/versions/latest'}
    request.update({'timeout':1})
    client.access_secret_version(request=request)
def OpaqueAlias(flag):
    client=secretmanager.SecretManagerServiceClient()
    request={'name':'projects/review/secrets/stale-opaque/versions/latest'}
    other={'name':'x'}
    alias=request
    if flag:
        alias=other
    alias['name']='after'
    client.access_secret_version(request=request)
