import { EventHubProducerClient, EventHubConsumerClient } from '@azure/event-hubs';
import { BlobServiceClient } from '@azure/storage-blob';
import { SecretClient } from '@azure/keyvault-secrets';
const storage = new BlobServiceClient(endpoint, credential);
const secrets = new SecretClient(vault, credential);
export function send() { new EventHubProducerClient(connection,process.env.HUB).sendBatch(batch); }
export function receive() { new EventHubConsumerClient(group,connection,process.env.HUB).subscribe(handler); }
export function write() { storage.getContainerClient(process.env.CONTAINER).getBlockBlobClient(unknownObject).uploadData(data); }
export function read() { storage.getContainerClient(process.env.CONTAINER).getBlobClient(unknownObject).download(); }
export function secret() { secrets.getSecret(process.env.SECRET); }
export function wrongHub() { new EventHubProducerClient(connection,process.env.WRONG).sendBatch(batch); }
export function wrongBlob() { storage.getContainerClient(process.env.WRONG).getBlobClient(unknownObject).download(); }
export function wrongSecret() { secrets.getSecret(process.env.WRONG); }
export function missingSecret() { secrets.getSecret(process.env.MISSING); }
export function privateSecret() { secrets.getSecret(process.env.PRIVATE_SECRET); }
