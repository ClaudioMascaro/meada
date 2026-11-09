using System.Threading.Tasks;
using Env = System.Environment;
using Amazon.S3;
using Amazon.S3.Model;
class Aliases {
    async Task Use() {
        var client = new AmazonS3Client();
        await client.PutObjectAsync(new PutObjectRequest { BucketName = Env.GetEnvironmentVariable("CS_BUCKET"), Key = "row" });
        await client.GetObjectAsync(new GetObjectRequest { BucketName = Env.GetEnvironmentVariable("CS_BUCKET"), Key = "row" });
        var request = new GetObjectRequest { BucketName = "stale", Key = "row" };
        var alias = request;
        alias.BucketName = "changed";
        await client.GetObjectAsync(request);
    }
}
