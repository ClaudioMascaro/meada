using System;
using System.Collections.Generic;
using Amazon.SimpleSystemsManagement;
using Amazon.SimpleSystemsManagement.Model;

class Batches {
    async Task Read() {
        var ssm = new AmazonSimpleSystemsManagementClient();
        await ssm.GetParametersAsync(new GetParametersRequest {
            Names = new List<string> { "/orders/key", Environment.GetEnvironmentVariable("PARAM") }
        });
    }
}
