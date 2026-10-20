using System.Net;
using System.Reflection;
using System.Text;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class ConnectionUpgradeChecks
{
    internal static async Task Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"ultros-upgrade-"+Guid.NewGuid().ToString("N"));
        try {
            var endpoint=new UltrosEndpoint(false);
            using var handler=new RefreshHandler();
            using var connection=new Connection(root,endpoint,"fixture",handler);
            var credentials=new ProtectedCredentials(connection.Directory,endpoint,"fixture");
            const string token="ult_r1.original-fixture";
            credentials.Save(token);
            var path=Path.Combine(connection.Directory,"refresh.protected.json");
            var saved=File.Exists(path)?File.ReadAllBytes(path):null;
            if(credentials.CanPersist) {
                await connection.Initialize(CancellationToken.None);
                if(saved is null||!File.ReadAllBytes(path).SequenceEqual(saved))throw new Exception("HTTP 426 removed or replaced the protected refresh credential.");
            }
            else {
                // Wine/memory-only fixtures still exercise the renewal exception path.
                SetRefresh(connection,token);
                try{_=await connection.Send<bool>(HttpMethod.Get,"/fixture",null,CancellationToken.None);throw new Exception("HTTP 426 accepted.");}
                catch(ApiFailure error) when(error.Status==426){}
            }
            if(handler.Revocations!=0)throw new Exception("Compatibility rejection revoked an unconsumed refresh token.");
            handler.Uncertain=true;SetRefresh(connection,token);
            try{_=await connection.Send<bool>(HttpMethod.Get,"/fixture",null,CancellationToken.None);throw new Exception("Uncertain refresh accepted.");}
            catch(HttpRequestException){}
            if(handler.Revocations!=1||File.Exists(path))throw new Exception("Uncertain refresh stopped failing closed.");
            handler.Uncertain=false;handler.IssueBeforeRejection=true;SetRefresh(connection,token);
            try{_=await connection.Send<bool>(HttpMethod.Get,"/fixture",null,CancellationToken.None);throw new Exception("Unverified issuance accepted.");}
            catch(ApiFailure error) when(error.Status==426){}
            if(handler.Revocations<2||File.Exists(path))throw new Exception("Post-issuance verification failure retained an uncertain credential.");
        }
        finally{if(Directory.Exists(root))Directory.Delete(root,true);}
    }
    private static void SetRefresh(Connection connection,string token)=>typeof(Connection)
        .GetField("refresh",BindingFlags.Instance|BindingFlags.NonPublic)!.SetValue(connection,token);
    private sealed class RefreshHandler:HttpMessageHandler
    {
        internal int Revocations;
        internal bool Uncertain,IssueBeforeRejection;
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellationToken)
        {
            if(request.RequestUri!.AbsolutePath.EndsWith("/revoke",StringComparison.Ordinal)) {
                Revocations++;return Task.FromResult(new HttpResponseMessage(HttpStatusCode.NoContent));
            }
            if(request.RequestUri.AbsolutePath.EndsWith("/capabilities",StringComparison.Ordinal))
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK){Content=new StringContent("{\"protocol\":1}",Encoding.UTF8,"application/json")});
            if(Uncertain)throw new HttpRequestException("Fixture response uncertainty.");
            if(IssueBeforeRejection&&request.RequestUri.AbsolutePath.EndsWith("/refresh",StringComparison.Ordinal))
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK){Content=new StringContent(
                    "{\"token_type\":\"Bearer\",\"access_token\":\"ult_a1.fixture\",\"refresh_token\":\"ult_r1.rotated-fixture\",\"expires_in\":900,\"account_id\":\"1\",\"device_id\":\"fixture\"}",Encoding.UTF8,"application/json")});
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.UpgradeRequired){Content=new StringContent("{}",Encoding.UTF8,"application/json")});
        }
    }
}
