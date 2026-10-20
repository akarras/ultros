using System.Net;
using System.Text;
using System.Text.Json;
using Ultros.Client;
namespace Ultros.Lists.Smoke;
internal static class ClientChecks
{
    private static void Require(bool value,string failure){if(!value)throw new Exception(failure);}
    private sealed class ChunkedEvents(byte[] data):MemoryStream(data)
    {
        public override ValueTask<int> ReadAsync(Memory<byte> buffer,CancellationToken cancellationToken=default)=>base.ReadAsync(buffer[..Math.Min(3,buffer.Length)],cancellationToken);
    }
    internal static async Task Run()
    {
        Require(Transport.ResponseLimit("/api/v1/plugin/lists/1/sync") == 24 * 1024 * 1024,
            "List sync does not allow its bounded combined version/snapshot envelope.");
        Require(Transport.ResponseLimit("/api/v1/plugin/lists") == 12 * 1024 * 1024
            && Transport.ResponseLimit("/api/v1/plugin/notifications") == 12 * 1024 * 1024,
            "List sync enlarged unrelated HTTP response limits.");
        await TransportDeadlineChecks.Run();
        await ConnectionUpgradeChecks.Run();
        ListEditTargetChecks.Run();
        InventoryProgressChecks.Run();
        RetainerPriceChecks.Run();
        var observationTime=DateTimeOffset.UtcNow;
        var observedCharacter=new ObservedCharacter("123456789","Same Name",63,[]);
        var oldCharacter=new UnverifiedCharacter("1","Same Name",63,true,false,false,0,observationTime.AddDays(-1),[]);
        var currentCharacter=oldCharacter with {Id="2",ObservedAt=observationTime};
        var anotherWorld=currentCharacter with {Id="3",HomeWorld=64,ObservedAt=observationTime.AddMinutes(1)};
        Require(ClientWorker.LatestObservation([oldCharacter,anotherWorld,currentCharacter],observedCharacter)?.Id=="2",
            "Registration selected an old same-name roster from a previous development origin.");
        var production=new UltrosEndpoint(false);
        var verificationUrl=production.At("/plugin/verify").AbsoluteUri;
        var linkToken=new string('A',43);
        var deviceCode=new string('B',43);
        var complete=verificationUrl+"#pair="+linkToken;
        Require(Connection.ValidApprovalLink(complete,verificationUrl,deviceCode),"A valid direct approval link was rejected.");
        foreach(var invalid in new[]{"https://attacker.invalid/plugin/verify#pair="+linkToken,
            verificationUrl+"?token="+linkToken+"#pair="+linkToken,verificationUrl+"#pair="+deviceCode,
            verificationUrl+"#pair="+new string('A',42)+"B",verificationUrl+"#pair=ABCD-EFGH"})
            Require(!Connection.ValidApprovalLink(invalid,verificationUrl,deviceCode),"An unsafe approval link was accepted.");
        var pairingView=new ClientView(false,"","","ABCD-EFGH",verificationUrl,null,[],null,null,false,[],[],null,[],null,[],ApprovalUrl:complete);
        Require(!pairingView.ToString().Contains(linkToken),"The UI snapshot exposed a pairing link in logs.");
        Require(!new PairingChallenge{VerificationUriComplete=complete}.ToString().Contains(linkToken),"The challenge exposed its pairing link in logs.");
        Require(production.Origin.AbsoluteUri=="https://ultros.app/","Production origin changed.");
        foreach(var invalid in new[]{"//attacker.invalid/token","/\\attacker.invalid","https://attacker.invalid"}) {
            try{production.At(invalid);throw new Exception("Cross-origin path accepted.");}catch(ArgumentException){}
        }
        if(UltrosEndpoint.LocalhostAvailable)
        {
            foreach(var invalid in new[]{"http://localhost.evil.invalid","http://192.168.1.1","http://localhost/path","http://name:pass@localhost","http://127.0.0.1/?code=x"})
                try{_=new UltrosEndpoint(true,invalid);throw new Exception("Non-loopback development target accepted.");}catch(ArgumentException){}
            Require(new UltrosEndpoint(true,"http://[::1]:8080").IsLocalhost,"IPv6 loopback rejected.");
        }
        else {
            try{_=new UltrosEndpoint(true);throw new Exception("Release enabled localhost implicitly.");}catch(InvalidOperationException){}
        }
        var handler=new ProbeHandler();
        using(var transport=new Transport(production,handler))
        {
            _=await transport.Send<bool>(HttpMethod.Post,"/api/v1/plugin/session/revoke",new{refresh_token="refresh-test-secret"},"access-test-secret",CancellationToken.None);
            Require(handler.Authorization=="Bearer access-test-secret","Access token missing.");
            Require(handler.ClientVersion==ClientBuild.Version.ToString(4)&&handler.ProtocolVersion=="1","Release version headers missing.");
            _=await transport.Send<JsonDocument>(HttpMethod.Get,"/api/v1/listings/Gilgamesh/8",null,null,CancellationToken.None);
            Require(handler.Authorization is null,"Resource request leaked an authorization header to the public call.");
            handler.Status=HttpStatusCode.BadRequest;handler.Body="{\"error\":\"refresh-test-secret\"}";
            try{_=await transport.Send<JsonDocument>(HttpMethod.Get,"/fixture",null,null,CancellationToken.None);throw new Exception("Failure accepted.");}
            catch(ApiFailure e){Require(!e.Message.Contains("secret")&&e.Code=="request_failed","Error exposed raw response content.");}
            handler.Status=HttpStatusCode.Found;handler.Body="";
            try{_=await transport.Send<bool>(HttpMethod.Post,"/fixture",new{},null,CancellationToken.None);throw new Exception("Redirect accepted.");}
            catch(ApiFailure e){Require(e.Status==302,"Redirect was followed.");}
            handler.Status=HttpStatusCode.OK;handler.Body=new string('x',12*1024*1024+1);
            try{_=await transport.Send<JsonDocument>(HttpMethod.Get,"/fixture",null,null,CancellationToken.None);throw new Exception("Unbounded response accepted.");}
            catch(IOException){}
        }
        Require(!new TokenPair{RefreshToken="secret"}.ToString().Contains("secret"),"Credentials implement an unsafe ToString.");
        using(var supported=JsonDocument.Parse("{\"protocol\":1,\"minimum_client_version\":\"1.0.0.0\"}"))
            ClientBuild.CheckCapabilities(supported.RootElement);
        foreach(var policy in new[]{"{\"protocol\":2}","{\"protocol\":1,\"minimum_client_version\":\"65534.0.0.0\"}"})
        {
            using var unsupported=JsonDocument.Parse(policy);
            try{ClientBuild.CheckCapabilities(unsupported.RootElement);throw new Exception("Unsupported server policy accepted.");}
            catch(ApiFailure e){Require(e.Status==426&&e.Message.Contains("Update Ultros"),"Outdated client lacks update instructions.");}
        }
        var worldDirectory=new WorldDirectory([new(1,"Japan",[new(1,"Elemental",[new(72,"Tonberry")])]),new(2,"North-America",[new(2,"Aether",[new(63,"Gilgamesh")])])]);
        Require(worldDirectory.RegionFor(72,63)=="Japan","Current region did not take precedence over home world.");
        Require(worldDirectory.RegionFor(0,63)=="North-America","Missing current world did not fall back to home region.");
        Require(worldDirectory.RegionFor(999,998)=="","Unknown worlds silently chose a different price zone.");
        ListAccess List(int id,string name,string permission)=>new(new(id,1,name,default),permission,null);
        var selection=new[]{List(1,"Wishlist","Owner"),List(2,"Cart","Read"),List(3,"cart","Owner")};
        Require(ListSelection.Default(selection,null)==3,"Default cart selected a shared list over the owned Cart.");
        Require(ListSelection.Default(selection,1)==1,"Explicit list selection was not preserved.");
        Require(ListSelection.Default(selection,999)==3,"Missing remembered list did not fall back to Cart.");
        var clamped=new[]{new ListAccess(new(4,2,"Cart",default),"Write",null),new ListAccess(new(5,1,"Cart",default),"Write",null)};
        Require(ListSelection.Default(clamped,null,"1")==5,"Default Cart ignored ownership when permissions were clamped to Write.");
        var wakes=0;
        using(var events=new ChunkedEvents(Encoding.UTF8.GetBytes(": heartbeat\r\n\r\nevent: notifications\r\ndata: ready\r\n\r\nevent: other\n\nevent: notifications\ndata: ready\n\nevent: notifications\ndata: truncated")))
            await NotificationFeed.Read(events,()=>wakes++,CancellationToken.None);
        Require(wakes==2,"SSE fragmentation, comments, CRLF or truncated frames dispatched incorrectly.");
        using(var oversized=new MemoryStream(Encoding.UTF8.GetBytes("data: "+new string('x',1100))))
        {
            try{await NotificationFeed.Read(oversized,()=>{},CancellationToken.None);throw new Exception("Oversized SSE line accepted.");}catch(IOException){}
        }

        var root=Path.Combine(Path.GetTempPath(),"ultros-credential-check-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            var a=Path.Combine(root,"a");var b=Path.Combine(root,"b");
            var store=new ProtectedCredentials(a,production,"a");
            store.Save("refresh-roundtrip-test-secret");
            var path=Path.Combine(a,"refresh.protected.json");
            if(store.CanPersist)
            {
                Require(!File.ReadAllText(path).Contains("refresh-roundtrip-test-secret"),"Refresh credential is plaintext.");
                Require(store.Load()=="refresh-roundtrip-test-secret","DPAPI roundtrip failed.");
                var other=new ProtectedCredentials(b,production,"b");File.Copy(path,Path.Combine(b,"refresh.protected.json"));
                try{_=other.Load();throw new Exception("A different profile decrypted the credential.");}
                catch(System.Security.Cryptography.CryptographicException){}
            }
            else Require(!File.Exists(path)&&store.Load() is null,"Unsupported credential backend persisted a credential.");
            store.Clear();
        }
        finally
        {
            foreach(var directory in Directory.EnumerateDirectories(root)){foreach(var file in Directory.EnumerateFiles(directory))File.Delete(file);Directory.Delete(directory);}
            Directory.Delete(root);
        }

        var gate=new CaptureGate();var now=DateTimeOffset.UtcNow;
        const string sensitiveLink="https://ultros.app/plugin/verify#pair=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        Require(!await BrowserLinks.Open(sensitiveLink,_=>throw new InvalidOperationException("A launch error containing "+sensitiveLink)),"Browser errors must return only a failure flag.");
        Require(await BrowserLinks.Open(sensitiveLink,start=>start.FileName==sensitiveLink&&start.UseShellExecute&&start.Verb=="open"),"Browser links must be passed directly to the OS shell.");
        Require(!await BrowserLinks.Open("file:///C:/Windows/test",_=>true),"Only HTTP(S) links may be opened.");
        var rows=new[]{new PrivateListing(0,8,false,5,100)};
        Require(gate.Observe("visit-a",false,true,1,rows,now) is null,"First cached view counted as fresh.");
        Require(gate.Observe("visit-a",false,true,1,rows,now.AddSeconds(1)) is null,"No market update counted as fresh.");
        Require(gate.Observe("visit-a",true,false,1,rows,now.AddSeconds(2)) is null,"Unloaded inventory accepted.");
        Require(gate.Observe("visit-a",false,true,1,rows,now.AddSeconds(3)) is null,"Debounce skipped.");
        Require(gate.Observe("visit-a",false,true,1,rows,now.AddMilliseconds(3400)) is null,"Debounce too short.");
        Require(gate.Observe("visit-a",false,true,1,rows,now.AddMilliseconds(3600)) is not null,"Stable complete capture missing.");
        gate.Submitted(rows);
        Require(gate.Observe("visit-a",false,true,1,rows,now.AddSeconds(5)) is null,"Identical capture submitted twice.");
        var changed=new[]{rows[0] with {UnitPrice=90}};
        Require(gate.Observe("visit-a",false,true,1,changed,now.AddSeconds(6)) is null,"Price-only change skipped debounce.");
        Require(gate.Observe("visit-a",false,true,1,changed,now.AddSeconds(7)) is not null,"Price-only change was ignored.");
        Require(gate.Observe("visit-b",false,true,0,[],now.AddSeconds(8)) is null,"Retainer switch reused freshness.");
        Require(gate.Observe("visit-b",false,true,0,[],now.AddSeconds(9)) is null,"Unloaded empty inventory cleared listings.");
        gate.Observe("visit-b",true,true,0,[],now.AddSeconds(10));
        Require(gate.Observe("visit-b",false,true,0,[],now.AddSeconds(11)) is not null,"A fresh loaded empty selling list must be uploadable.");
        gate.Reset();Require(gate.Observe("visit-b",false,true,0,[],now.AddSeconds(12)) is null,"Logout reused a visit.");
        var opening=new CaptureGate();
        Require(opening.Observe("opening",true,true,1,rows,now) is null,"Opening skipped stability debounce.");
        Require(opening.Observe("opening",false,true,1,rows,now.AddMilliseconds(600)) is not null,"The update opening the selling window was discarded.");
        var freshness=new CaptureFreshness();
        freshness.Signal("character/retainer/object",now);
        Require(freshness.Take("character/retainer/object",now.AddSeconds(1)),"An update before window readiness was lost.");
        Require(!freshness.Take("character/retainer/object",now.AddSeconds(2)),"A market signal was consumed twice.");
        freshness.Signal("previous",now);
        Require(!freshness.Take("other",now.AddSeconds(1))&&!freshness.Take("previous",now.AddSeconds(2)),"A retainer switch reused an earlier signal.");
        freshness.Signal("expired",now);Require(!freshness.Take("expired",now.AddSeconds(11)),"An expired update was accepted.");
        Console.WriteLine("PASS: fixed origins, redacted transport failures, bounded responses, protected credential roundtrip/profile isolation, and retainer freshness/debounce/empty-list guards.");
    }
    private sealed class ProbeHandler : HttpMessageHandler
    {
        internal string? Authorization;
        internal string? ClientVersion,ProtocolVersion;
        internal HttpStatusCode Status=HttpStatusCode.OK;
        internal string Body="{}";
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellation)
        {
            Authorization=request.Headers.Authorization?.ToString();
            ClientVersion=request.Headers.TryGetValues("X-Ultros-Client-Version",out var versions)?versions.Single():null;
            ProtocolVersion=request.Headers.TryGetValues("X-Ultros-Protocol-Version",out var protocols)?protocols.Single():null;
            return Task.FromResult(new HttpResponseMessage(Status){Content=new StringContent(Body,Encoding.UTF8,"application/json")});
        }
    }
}
