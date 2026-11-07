using System;
using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;
using Amazon.SecretsManager;
using Amazon.SecretsManager.Model;

class Sends {
    async Task Use() {
        var sns = new AmazonSimpleNotificationServiceClient();
        await sns.PublishAsync(new PublishRequest { TopicArn = Environment.GetEnvironmentVariable("TOPIC") });
        await sns.PublishAsync(request: new PublishRequest { TopicArn = Environment.GetEnvironmentVariable("NAMED_TOPIC") });
        var secrets = new AmazonSecretsManagerClient();
        await secrets.GetSecretValueAsync(new GetSecretValueRequest { SecretId = "arn:aws:secretsmanager:us-east-1:123:secret:orders/token-a1B2c3" });
    }
}
