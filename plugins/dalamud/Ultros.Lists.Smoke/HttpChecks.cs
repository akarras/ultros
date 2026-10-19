using System.Diagnostics;
using System.Text.Json;
using Ultros.Client;
using Ultros.Lists;
namespace Ultros.Lists.Smoke;

internal static class HttpChecks
{
    private static void Require(bool condition,string message){if(!condition)throw new Exception(message);}
    internal static async Task Run(string origin)
    {
        var endpoint=new UltrosEndpoint(true,origin);
        var root=Path.Combine(Path.GetTempPath(),"ultros-http-check-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        using var cancellation=new CancellationTokenSource(TimeSpan.FromMinutes(2));
        using var connection=new Connection(root,endpoint,"one");
        try
        {
            await connection.BeginPairing("Managed HTTP integration",[..Protocol.NotificationScopes,"lists:read","lists:write","retainers:read","retainers:register","retainers:upload"],cancellation.Token);
            var start=new ProcessStartInfo("node"){UseShellExecute=false,CreateNoWindow=true,RedirectStandardInput=true,RedirectStandardOutput=true,RedirectStandardError=true};
            start.ArgumentList.Add(Path.GetFullPath("integration/dalamud-approve.cjs"));
            using var browser=Process.Start(start)??throw new Exception("Browser approval helper did not start.");
            Require(connection.ApprovalUrl.StartsWith(connection.VerificationUrl+"#pair=",StringComparison.Ordinal),"The server did not provide a direct approval link.");
            await browser.StandardInput.WriteAsync(JsonSerializer.Serialize(new{origin,code=connection.HumanCode,approval_url=connection.ApprovalUrl}));
            browser.StandardInput.Close();
            var outputTask=browser.StandardOutput.ReadToEndAsync(cancellation.Token);
            var errorTask=browser.StandardError.ReadToEndAsync(cancellation.Token);
            await browser.WaitForExitAsync(cancellation.Token);
            var output=await outputTask;var error=await errorTask;
            Require(browser.ExitCode==0,"Browser approval failed: "+error);
            using var browserResult=JsonDocument.Parse(output);
            var id=browserResult.RootElement.GetProperty("list_id").GetInt32();
            for(var i=0;i<40&&!connection.Connected;i++){await Task.Delay(250,cancellation.Token);await connection.PollPairing(cancellation.Token);}
            Require(connection.Connected&&connection.Session?.AccountId=="900000000000000112","Managed pairing did not establish the approved account.");
            try{using var other=new Connection(root,endpoint,"one");throw new Exception("A concurrent process acquired the same credential profile.");}catch(IOException){}
            var lists=await connection.Send<ListAccess[]>(HttpMethod.Get,"/api/v1/plugin/lists",null,cancellation.Token);
            Require(lists.Any(l=>l.List.Id==id&&l.CanWrite),"The approved list writer could not see its cart.");
            using(var cart=new ConnectedCart(connection,id){CanWrite=true})
            {
                await cart.Sync(connection,cancellation.Token);
                Require(cart.Document.View.Ready,"Initial snapshot did not establish the online document.");
                cart.Edit(new{kind="add",item_id=8,quality="nq",need=10});
                await cart.Sync(connection,cancellation.Token);
                using var concurrent=Document.Open(cart.Document.Export());
                concurrent.Edit(new{kind="add_acquired",item_id=8,quality="nq",delta=3});
                cart.Edit(new{kind="add_acquired",item_id=8,quality="nq",delta=2});
                var request=new SyncRequest(cart.Document.View.Version,Convert.ToBase64String(concurrent.Export()));
                _=await connection.Send<SyncReply>(HttpMethod.Post,$"/api/v1/plugin/lists/{id}/sync",request,cancellation.Token);
                _=await connection.Send<SyncReply>(HttpMethod.Post,$"/api/v1/plugin/lists/{id}/sync",request,cancellation.Token);
                await cart.Sync(connection,cancellation.Token);
                Require(cart.Document.View.Rows.Single().Acquired==5,"Concurrent C# peers did not converge once per original operation.");
                var estimate=await connection.Send<CartEstimate>(HttpMethod.Get,$"/api/v1/plugin/lists/{id}/estimate",null,cancellation.Token);
                Require(estimate.UnpricedUnits>=0,"Shared Rust market estimate has a negative missing quantity.");
                Require(estimate.Version==cart.Document.View.Version,"Shared Rust market estimate has a different document version.");
            }
            using(var restored=new ConnectedCart(connection,id){CanWrite=true}){Require(restored.Document.View.Rows.Single().Acquired==5,"Managed checkpoint did not survive reopening.");await restored.Sync(connection,cancellation.Token);}
            var operation=Guid.NewGuid().ToString("N");
            using var notifications=new SemaphoreSlim(0);
            using var feed=await connection.SubscribeNotifications(()=>notifications.Release(),cancellation.Token)
                ??throw new Exception("The server did not advertise its notification stream.");
            Require(await notifications.WaitAsync(TimeSpan.FromSeconds(10),cancellation.Token),"Authenticated SSE stream did not provide its initial recovery wake-up.");
            _=await connection.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/test",new{operation_id=operation},cancellation.Token);
            Require(await notifications.WaitAsync(TimeSpan.FromSeconds(5),cancellation.Token),"Queued notification did not wake the SSE subscriber.");
            var batch=await connection.Send<DeliveryBatch>(HttpMethod.Get,"/api/v1/plugin/notifications",null,cancellation.Token);
            Require(batch.Deliveries.Length==1,"Game endpoint test did not yield one delivery.");
            _=await connection.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/ack",new{ids=batch.Deliveries.Select(d=>d.Id).ToArray()},cancellation.Token);
            feed.Dispose();
            try{await feed.Completion.WaitAsync(TimeSpan.FromSeconds(5),cancellation.Token);}catch(OperationCanceledException){}
            using(var reopened=await connection.SubscribeNotifications(()=>notifications.Release(),cancellation.Token)
                ??throw new Exception("Notification stream could not reconnect."))
            {
                Require(await notifications.WaitAsync(TimeSpan.FromSeconds(5),cancellation.Token),"Reconnected SSE stream did not request recovery.");
                var recovered=await connection.Send<DeliveryBatch>(HttpMethod.Get,"/api/v1/plugin/notifications",null,cancellation.Token);
                Require(recovered.Deliveries.Length==0,"Reconnecting replayed an acknowledged delivery.");
                reopened.Dispose();try{await reopened.Completion.WaitAsync(TimeSpan.FromSeconds(5),cancellation.Token);}catch(OperationCanceledException){}
            }
            await connection.Disconnect();
            Require(!connection.Connected&&!File.Exists(Path.Combine(connection.Directory,"refresh.protected.json")),"Disconnect retained a credential.");
            Console.WriteLine("PASS: real C# pairing/browser consent, scoped HTTP transport, exclusive credential profile, list checkpoint/concurrent sync, shared Rust estimate, SSE wake-up/reconnect/recovery, notifications and disconnect.");
        }
        finally
        {
            if(connection.Connected)await connection.Disconnect();
            connection.Dispose();
            var full=Path.GetFullPath(root);
            var prefix=Path.GetFullPath(Path.Combine(Path.GetTempPath(),"ultros-http-check-"));
            if(!full.StartsWith(prefix,StringComparison.OrdinalIgnoreCase))throw new IOException("Unexpected cleanup path.");
            Directory.Delete(full,true);
        }
    }
}
