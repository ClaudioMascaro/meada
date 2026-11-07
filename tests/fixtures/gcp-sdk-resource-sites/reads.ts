import {Storage} from '@google-cloud/storage';
import {SecretManagerServiceClient} from '@google-cloud/secret-manager';
const storage=new Storage();
const secrets=new SecretManagerServiceClient();
export function readTwice() {
  storage.bucket('double-reads').file('object').download();
  storage.bucket('double-reads').file('object').download();
  secrets.accessSecretVersion({name:'projects/review/secrets/double-token/versions/latest'});
  secrets.accessSecretVersion({name:'projects/review/secrets/double-token/versions/latest'});
}
