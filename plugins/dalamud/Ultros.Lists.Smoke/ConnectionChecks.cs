using System.Net;
using System.Text.Json;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class ConnectionChecks
{
    private static void Require(bool condition,string message) { if(!condition)throw new Exception(message); }
    private static TokenPair Pair(int generation)=>new(){TokenType="Bearer",AccessToken=$"ult_a1.fixture.{generation}",RefreshToken=$"ult_r1.fixture.{generation}",DeviceId="fixture-device",AccountId="123",AccountName="Fixture",ExpiresIn=900,Scopes=["lists:read"]};
    private sealed class Handler : HttpMessageHandler
    {
        internal bool Offline,FailVerification,LoseRotation,WrongIdentity;
        internal CancellationTokenSource? CancelRotation;
        internal int Rotations,Revocations,Generation=1;
        internal string? LastRefresh;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellation)
        {
            object body;
            switch(request.RequestUri!.AbsolutePath) {
                case "/api/v1/plugin/capabilities":
                    if(Offline)return new(HttpStatusCode.ServiceUnavailable){Content=new StringContent("{}")};
                    body=new{protocol=ClientBuild.ServerProtocol};break;
                case "/api/v1/plugin/session/refresh":
                    Rotations++;
                    using(var content=JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellation)))LastRefresh=content.RootElement.GetProperty("refresh_token").GetString();
                    if(CancelRotation is { } source){source.Cancel();cancellation.ThrowIfCancellationRequested();}
                    if(LoseRotation)throw new HttpRequestException("Synthetic lost response");
                    body=Pair(++Generation);break;
                case "/api/v1/plugin/session/revoke":Revocations++;return new(HttpStatusCode.NoContent){Content=new StringContent("")};
                case "/api/v1/plugin/session":
                    if(FailVerification)return new(HttpStatusCode.ServiceUnavailable){Content=new StringContent("{}")};
                    var now=DateTimeOffset.UtcNow;
                    body=new DeviceSession("fixture-device","Fixture",WrongIdentity?"456":"123","Fixture",["lists:read"],now,now,now.AddDays(1),null);break;
                default:throw new Exception("Unexpected fixture request.");
            }
            return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(body,Protocol.Json))};
        }
    }
    internal static async Task Run()
    {
        var root=Path.GetFullPath(Path.Combine(Path.GetTempPath(),"ultros-connection-checks-"+Guid.NewGuid().ToString("N")));
        Directory.CreateDirectory(root);
        try
        {
            var endpoint=new UltrosEndpoint(false);
            var handler=new Handler();
            using(var connection=new Connection(root,endpoint,"restore",handler))
            {
                await connection.StoreIssued(Pair(1),null,CancellationToken.None);
                var path=Path.Combine(connection.Directory,"refresh.protected.json");
                var persisted=File.Exists(path);var before=persisted?File.ReadAllBytes(path):[];
                handler.Offline=true;
                try {await connection.Renew(CancellationToken.None);throw new Exception("Unavailable preflight succeeded.");}catch(ApiFailure e)when(e.Status==503){}
                Require(connection.CanRestore&&!connection.Connected&&handler.Rotations==0&&handler.Revocations==0,"Preflight outage consumed or revoked the saved login.");
                if(persisted)Require(before.SequenceEqual(File.ReadAllBytes(path)),"Preflight outage changed the saved credential.");
                handler.Offline=false;
                Require(await connection.Restore(CancellationToken.None,true),"Saved login did not restore after the preflight outage.");
                Require(handler.Rotations==1&&handler.LastRefresh==Pair(1).RefreshToken,"Restoration did not use the original unconsumed generation.");
                handler.FailVerification=true;
                try {await connection.Renew(CancellationToken.None);throw new Exception("Unavailable verification succeeded.");}catch(ApiFailure e)when(e.Status==503){}
                Require(connection.CanRestore&&!connection.Connected&&handler.Rotations==2&&handler.Revocations==0,"Verification outage lost the known new generation.");
                if(persisted)Require(File.Exists(path),"Verification outage deleted the new saved credential.");
                handler.FailVerification=false;
                Require(await connection.Restore(CancellationToken.None,true),"Pending session verification did not recover.");
                Require(handler.Rotations==2,"Session GET retry rotated again instead of verifying the known issued pair.");
                handler.LoseRotation=true;
                try {await connection.Renew(CancellationToken.None);throw new Exception("Lost rotation response succeeded.");}catch(HttpRequestException){}
                Require(!connection.CanRestore&&!connection.Connected&&handler.Revocations==1&&!File.Exists(path),"An ambiguous rotation retained a possibly consumed credential.");
                handler.LoseRotation=false;
                await connection.StoreIssued(Pair(10),null,CancellationToken.None);
                using(var cancelled=new CancellationTokenSource()) {
                    cancelled.Cancel();var sent=handler.Rotations;
                    try {await connection.Renew(cancelled.Token);throw new Exception("Cancelled preflight succeeded.");}catch(OperationCanceledException){}
                    Require(connection.CanRestore&&handler.Rotations==sent,"Pre-send cancellation discarded the unconsumed login.");
                }
                using(var during=new CancellationTokenSource()) {
                    handler.CancelRotation=during;
                    try {await connection.Renew(during.Token);throw new Exception("Cancelled rotation succeeded.");}catch(OperationCanceledException){}
                    Require(!connection.CanRestore&&handler.Revocations==2,"During-send cancellation retained an uncertain generation.");
                    handler.CancelRotation=null;
                }
                handler.WrongIdentity=true;
                try {await connection.StoreIssued(Pair(20),null,CancellationToken.None);throw new Exception("Changed identity was accepted.");}catch(InvalidOperationException){}
                Require(!connection.CanRestore&&!connection.Connected&&!File.Exists(path),"Identity rejection retained the issued grant.");
            }
            using(var owner=new Connection(root,endpoint,"reload"))
            using(var worker=new ClientWorker(root,endpoint,"reload"))
            {
                for(var attempt=0;attempt<100&&!worker.View.Status.StartsWith("Waiting for the previous",StringComparison.Ordinal);attempt++)await Task.Delay(50);
                Require(worker.View.Status.StartsWith("Waiting for the previous",StringComparison.Ordinal),"Reload did not wait for the previous profile owner.");
                owner.Dispose();
                for(var attempt=0;attempt<100&&worker.View.StorageMode.Length==0;attempt++)await Task.Delay(50);
                Require(worker.View.StorageMode.Length>0,"Reload did not acquire the profile after cleanup released it.");
            }
        }
        finally
        {
            Require(Path.GetDirectoryName(root)==Path.GetFullPath(Path.GetTempPath()).TrimEnd(Path.DirectorySeparatorChar),"Refusing unexpected temporary directory cleanup.");
            Directory.Delete(root,true);
        }
    }
}
