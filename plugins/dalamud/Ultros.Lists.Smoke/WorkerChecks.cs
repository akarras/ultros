using System.Net;
using System.Collections.Concurrent;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Threading.Channels;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class WorkerChecks
{
    private static void Require(bool value,string message){if(!value)throw new Exception(message);}
    private static void Set(object target,string field,object value)=>target.GetType().GetField(field,BindingFlags.Instance|BindingFlags.NonPublic)!.SetValue(target,value);
    private static async Task Until(Func<bool> ready)
    {
        using var limit=new CancellationTokenSource(TimeSpan.FromSeconds(10));
        while(!ready())await Task.Delay(10,limit.Token);
    }
    private sealed class Handler : HttpMessageHandler
    {
        internal readonly List<string> Bodies=[];
        internal readonly List<string> NotificationReads=[];
        internal readonly List<string[]> PairingScopes=[];
        internal readonly List<ObservedCharacter> Registrations=[];
        internal readonly List<ObservedCharacter> Observations=[];
        internal UnverifiedCharacter[] RegisteredCharacters=[];
        internal RetainerSnapshot[] Snapshots=[];
        internal string[] Features=[];
        internal ListAccess[] Lists=[];
        internal readonly ConcurrentDictionary<int,SyncReply> SyncReplies=new();
        internal readonly ConcurrentDictionary<int,int> SyncCounts=new();
        internal bool ReadRoster;
        internal HttpStatusCode? Failure=HttpStatusCode.ServiceUnavailable;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellation)
        {
            var route=request.RequestUri!.AbsolutePath;
            if(route=="/api/v1/plugin/notifications/read")
            {
                NotificationReads.Add(await request.Content!.ReadAsStringAsync(cancellation));
                return new(HttpStatusCode.NoContent);
            }
            if(route=="/api/v1/plugin/capabilities")
                return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(new{protocol=1,features=Features}))};
            if(route.StartsWith("/api/v1/plugin/lists/",StringComparison.Ordinal)&&route.EndsWith("/sync",StringComparison.Ordinal)) {
                var id=int.Parse(route.Split('/')[5],System.Globalization.CultureInfo.InvariantCulture);
                SyncCounts.AddOrUpdate(id,1,(_,prior)=>prior+1);
                return SyncReplies.TryGetValue(id,out var reply)?new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(reply,Protocol.Json))}:new(HttpStatusCode.ServiceUnavailable){Content=new StringContent("{}")};
            }
            if(route=="/api/v1/plugin/session/reauthorize") {
                using var body=JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellation));
                PairingScopes.Add(body.RootElement.GetProperty("scopes").EnumerateArray().Select(value=>value.GetString()!).ToArray());
                return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(new {
                    device_code=new string('A',43),user_code="AAAA-AAAA",verification_uri="https://ultros.app/plugin/verify",expires_in=300,interval=5
                }))};
            }
            if(route=="/api/v1/plugin/retainers/register") {
                using var body=JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellation));
                Registrations.Add(body.RootElement.GetProperty("character").Deserialize<ObservedCharacter>(Protocol.Json)!);
                return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(RegisteredCharacters,Protocol.Json))};
            }
            if(route=="/api/v1/plugin/characters/observe") {
                using var body=JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellation));
                Observations.Add(body.RootElement.Deserialize<ObservedCharacter>(Protocol.Json)!);
                return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(RegisteredCharacters,Protocol.Json))};
            }
            if(request.Method==HttpMethod.Get&&request.RequestUri!.AbsolutePath=="/api/v1/plugin/lists")
                return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(Lists,Protocol.Json))};
            if(request.Method==HttpMethod.Get&&request.RequestUri!.AbsolutePath=="/api/v1/plugin/characters")
                return ReadRoster?new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(RegisteredCharacters,Protocol.Json))}:new(HttpStatusCode.ServiceUnavailable){Content=new StringContent("{}")};
            if(request.Method==HttpMethod.Get&&route=="/api/v1/plugin/retainers/snapshots")
                return new(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(Snapshots,Protocol.Json))};
            Require(request.RequestUri!.AbsolutePath=="/api/v1/plugin/retainers/1/listings","Unexpected fixture request.");
            Bodies.Add(await request.Content!.ReadAsStringAsync(cancellation));
            return new(Failure??HttpStatusCode.OK){Content=new StringContent(Failure is null?"{\"revision\":1,\"hash\":\"fixture\"}":"{\"error\":\"service_unavailable\"}",Encoding.UTF8,"application/json")};
        }
    }
    internal static async Task LiveLists()
    {
        using var fixture=new Fixture();await fixture.Connect();
        using var first=Document.Open();first.Edit(new{kind="add",item_id=8,quality="nq",need=10});
        using var second=Document.Open();second.Edit(new{kind="add",item_id=9,quality="nq",need=20});
        var handler=fixture.Handler;handler.Features=["list-stream"];
        handler.Lists=[new(new(1,123,"Cart",JsonSerializer.SerializeToElement<object?>(null)),"Owner",null),new(new(2,123,"Other",JsonSerializer.SerializeToElement<object?>(null)),"Owner",null)];
        handler.SyncReplies[1]=new(first.View.Version,"snapshot",Convert.ToBase64String(first.Export()));
        handler.SyncReplies[2]=new(second.View.Version,"snapshot",Convert.ToBase64String(second.Export()));
        var opened=new ConcurrentQueue<(int List,Action Wake,TaskCompletionSource End)>();
        var transport=(Transport)typeof(Connection).GetField("transport",BindingFlags.Instance|BindingFlags.NonPublic)!.GetValue(fixture.Connection)!;
        transport.ListListener=async(_,list,wake,cancellation)=>{
            var end=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            opened.Enqueue((list,wake,end));wake();await end.Task.WaitAsync(cancellation);
        };
        await fixture.OnWorker(()=>Set(fixture.Worker,"lists",handler.Lists));
        Require(fixture.Worker.SelectList(1),"Live list selection was not queued.");
        await Until(()=>opened.Count==1&&fixture.Worker.View.Cart?.Rows.Length==1&&fixture.Worker.View.ListTransport=="Live updates");
        var original=opened.First();
        await fixture.OnWorker(()=>Set(fixture.Worker,"nextCart",DateTimeOffset.UtcNow.AddHours(1)));
        first.Edit(new{kind="add",item_id=10,quality="hq",need=30});
        handler.SyncReplies[1]=new(first.View.Version,"snapshot",Convert.ToBase64String(first.Export()));
        var before=handler.SyncCounts[1];for(var i=0;i<200;i++)original.Wake();
        await Until(()=>fixture.Worker.View.Cart?.Rows.Length==2);
        Require(handler.SyncCounts[1]<=before+3,"A notification burst caused an unbounded sync storm.");
        Require(fixture.Worker.SelectList(2),"Second list selection was not queued.");
        await Until(()=>opened.Count>=2&&fixture.Worker.View.SelectedList==2&&fixture.Worker.View.Cart?.Rows.Single().ItemId==9);
        await fixture.OnWorker(()=>Set(fixture.Worker,"nextCart",DateTimeOffset.UtcNow.AddHours(1)));
        before=handler.SyncCounts[2];original.Wake();await Task.Delay(400);
        Require(handler.SyncCounts[2]==before,"An old list's wake crossed into the newly selected cart.");
        var current=opened.Last();current.End.SetResult();
        await Until(()=>fixture.Worker.View.ListTransport=="Periodic refresh");
        await Until(()=>opened.Count>=3&&fixture.Worker.View.ListTransport=="Live updates");
        await fixture.OnWorker(()=>{
            handler.Lists=[];fixture.SetSession(fixture.Session with{Id="new-list-device",AccountId="456",Scopes=[]});
        });
        await Until(()=>fixture.Worker.View.Session?.AccountId=="456"&&fixture.Worker.View.Cart is null);
        before=handler.SyncCounts.Values.Sum();original.Wake();current.Wake();await Task.Delay(400);
        Require(handler.SyncCounts.Values.Sum()==before,"A retired account's WebSocket caused another HTTP sync.");
        Console.WriteLine("PASS: immediate worker list refresh, burst coalescing, reconnect/fallback and list/account fencing.");
    }
    internal static async Task DefaultSetup()
    {
        using var fixture=new Fixture();await fixture.Connect();
        var worker=fixture.Worker;
        Require(worker.Pair("Default setup"),"Default pairing was not queued.");
        await Until(()=>worker.View.HumanCode is not null);
        string[] expected=["notifications:read","notifications:ack","notifications:mark-read","lists:read","lists:write","retainers:read","retainers:register","retainers:upload"];
        Require(fixture.Handler.PairingScopes.Single().Order().SequenceEqual(expected.Order()),"A default connection still needs extra feature opt-ins.");
        Require(worker.CancelPairing(),"Pairing cancellation was not queued.");
        await Until(()=>worker.View.HumanCode is null);
        Require(worker.Pair("Read-only setup",false,false,false,false),"Custom pairing was not queued.");
        await Until(()=>worker.View.HumanCode is not null);
        Require(fixture.Handler.PairingScopes.Last().Order().SequenceEqual(new[]{"notifications:read","notifications:ack","notifications:mark-read","lists:read"}.Order()),"Customized permissions silently restored optional scopes.");
        worker.CancelPairing();await Until(()=>worker.View.HumanCode is null);
        Require(worker.Pair("Upload only",false,false,true,false),"Upload-only pairing was not queued.");
        await Until(()=>worker.View.HumanCode is not null);
        var uploadScopes=fixture.Handler.PairingScopes.Last();
        Require(uploadScopes.Contains("retainers:read")&&uploadScopes.Contains("retainers:upload")&&!uploadScopes.Contains("retainers:register"),"Upload permissions lost their read dependency or added registration.");
        worker.CancelPairing();await Until(()=>worker.View.HumanCode is null);

        var observation=new ObservedCharacter("111","Fixture",63,[new("222","Retainer",1)]);
        Require(!worker.Register(observation)&&fixture.Handler.Registrations.Count==0,"Automatic registration bypassed its approved scope.");
        var authorized=fixture.Session with{Scopes=expected};
        await fixture.OnWorker(()=>fixture.SetSession(authorized));
        await Until(()=>worker.View.Scopes.Contains("retainers:register"));
        var saved=worker.View.Characters.Single() with{UploadEnabled=false};
        fixture.Handler.RegisteredCharacters=[saved];
        Require(!ClientWorker.NeedsRegistration([saved],observation),"A paused character was treated as unsaved.");
        Require(ClientWorker.NeedsRegistration([saved with{CaptureEpoch=0}],observation),"An untouched old upload default was not upgraded.");
        Require(ClientWorker.NeedsRegistration([],observation),"The first summoning-bell visit does not save retainers.");
        Require(!ClientWorker.NeedsRegistration([],observation with{Retainers=[]}),"An unloaded roster was registered.");
        var expanded=observation with{Retainers=[..observation.Retainers,new("333","New retainer",1)]};
        Require(ClientWorker.NeedsRegistration([saved],expanded),"A newly observed retainer was ignored.");
        var conflict=saved with{Retainers=[..saved.Retainers,new("2","New retainer",1,false,0,DateTimeOffset.UtcNow,true)]};
        Require(!ClientWorker.NeedsRegistration([conflict],expanded),"A grouping conflict caused endless automatic registration attempts.");
        Require(worker.Register(observation),"Authorized registration was not queued.");
        await Until(()=>worker.View.RosterStatus.StartsWith("Saved",StringComparison.Ordinal));
        Require(fixture.Handler.Registrations.Count==1&&fixture.Handler.Registrations[0].ContentId==observation.ContentId
            &&!worker.View.Characters.Single().UploadEnabled,"Registration lost the observation or changed a saved pause.");
        var detailed=observation with{Retainers=[observation.Retainers[0] with{GameOrder=0,Available=true}]};
        Require(worker.Observe(detailed),"Legacy-compatible observation was not queued.");
        await Until(()=>fixture.Handler.Observations.Count==1);
        Require(fixture.Handler.Observations[0].Retainers[0].GameOrder is null,"An old server received unsupported roster fields.");
        await fixture.OnWorker(()=>Set(worker,"retainerPreferences",true));
        Require(worker.Observe(detailed),"Roster metadata observation was not queued.");
        await Until(()=>fixture.Handler.Observations.Count==2);
        Require(fixture.Handler.Observations[1].Retainers[0].GameOrder==0&&fixture.Handler.Registrations.Count==1,"Known metadata was re-registered instead of observed.");

        await fixture.OnWorker(()=>{
            Require(worker.Register(observation),"Bound registration was not queued.");
            Require(worker.Observe(detailed),"Bound observation was not queued.");
            fixture.SetSession(authorized with{Id="another-device",AccountId="456"});
        });
        await fixture.OnWorker(()=>{});
        Require(fixture.Handler.Registrations.Count==1&&fixture.Handler.Observations.Count==2,"A queued game observation crossed to a different account.");
        Console.WriteLine("PASS: default connection permissions, customized grants, automatic retainer registration decisions, saved pauses and account-bound registration.");
    }
    private sealed class Fixture : IDisposable
    {
        // Seed an approved grant in the real serialized worker. No game, test-auth
        // server, live credentials or production-only testing hooks are needed.
        internal readonly string Root=Path.Combine(Path.GetTempPath(),"ultros-worker-check-"+Guid.NewGuid().ToString("N"));
        internal readonly ClientWorker Worker;
        internal Connection Connection=null!;
        internal DeviceSession Session=null!;
        internal readonly Handler Handler=new();
        internal Fixture()=>Worker=new(Root,new(false),"worker");
        internal async Task OnWorker(Action action)
        {
            var completion=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            var queue=(Channel<Func<Task>>)typeof(ClientWorker).GetField("commands",BindingFlags.Instance|BindingFlags.NonPublic)!.GetValue(Worker)!;
            Require(queue.Writer.TryWrite(()=>{try{action();completion.SetResult();}catch(Exception e){completion.SetException(e);}return Task.CompletedTask;}),"Fixture queue was full.");
            await completion.Task.WaitAsync(TimeSpan.FromSeconds(10));
        }
        internal async Task Connect()
        {
            await Until(()=>Worker.View.Status=="Disconnected");
            await OnWorker(()=>{
                ((Connection)typeof(ClientWorker).GetField("connection",BindingFlags.Instance|BindingFlags.NonPublic)!.GetValue(Worker)!).Dispose();
                Connection=new(Root,new(false),"connected",Handler);
                var now=DateTimeOffset.UtcNow;
                Session=new("fixture-device","Fixture","123","Fixture",["lists:read","lists:write","retainers:read","retainers:upload"],now,now,now.AddDays(1),null);
                SetSession(Session);
                Set(Worker,"connection",Connection);Set(Worker,"loadedDevice",Session.Id);
                Set(Worker,"characters",new[]{new UnverifiedCharacter("1","Fixture",63,true,false,true,1,now,[new("1","Retainer",1,true,0,now,false,1)])});
                foreach(var field in new[]{"nextLists","nextNotifications","nextCart"})Set(Worker,field,now.AddDays(1));
            });
            await Until(()=>Worker.View.Connected&&Worker.View.Characters.Length==1);
        }
        internal void SetSession(DeviceSession session)
        {
            Set(Connection,"access","synthetic-worker-fixture");Set(Connection,"accessExpires",DateTimeOffset.UtcNow.AddMinutes(10));
            Set(Connection,"<Session>k__BackingField",session);
        }
        public void Dispose()
        {
            Worker.Dispose();
            var path=Path.GetFullPath(Root);
            Require(Path.GetDirectoryName(path)==Path.GetFullPath(Path.GetTempPath()).TrimEnd(Path.DirectorySeparatorChar)
                &&Path.GetFileName(path).StartsWith("ultros-worker-check-",StringComparison.Ordinal),"Unexpected fixture cleanup path.");
            Directory.Delete(path,true);
        }
    }
    internal static async Task Uploads()
    {
        using var fixture=new Fixture();await fixture.Connect();
        var worker=fixture.Worker;
        var capture=new ListingCapture(Guid.NewGuid().ToString("N"),0,1,DateTimeOffset.UtcNow,true,[new(0,8,false,5,100)]);
        var failed=await worker.Upload("1",capture)!.WaitAsync(TimeSpan.FromSeconds(10));
        Require(!failed.Saved&&failed.Retryable,"HTTP 503 was acknowledged or permanently discarded.");
        fixture.Handler.Failure=null;
        var saved=await worker.Upload("1",capture)!.WaitAsync(TimeSpan.FromSeconds(10));
        Require(saved.Saved,"A successful receipt was lost when the roster GET failed.");
        Require(fixture.Handler.Bodies.Count==2&&fixture.Handler.Bodies[0]==fixture.Handler.Bodies[1],"Retry changed its idempotency key/body.");
        Require(worker.View.Characters.Single().Retainers.Single().Revision==1,"The acknowledged revision was not published before completion.");
        Require(worker.View.RetainerSnapshots?.Single().Listings.Single().UnitPrice==100,"An acknowledged upload did not update the visible listings.");
        await fixture.OnWorker(()=>Set(worker,"characters",worker.View.Characters.Select(c=>c with{Retainers=c.Retainers.Select(r=>r with{Revision=2}).ToArray()}).ToArray()));
        var newer=new RetainerSnapshot("1",2,DateTimeOffset.UtcNow,DateTimeOffset.UtcNow,[new(0,8,false,5,90)]);
        await fixture.OnWorker(()=>Set(worker,"retainerSnapshots",new[]{newer}));
        Require((await worker.Upload("1",capture)!.WaitAsync(TimeSpan.FromSeconds(10))).Saved
            &&worker.View.Characters.Single().Retainers.Single().Revision==2,"An old duplicate receipt downgraded a newer known revision.");
        Require(worker.View.RetainerSnapshots?.Single().Listings.Single().UnitPrice==90,"An old receipt replaced newer displayed prices.");
        fixture.Handler.Failure=HttpStatusCode.Forbidden;
        var denied=await worker.Upload("1",capture)!.WaitAsync(TimeSpan.FromSeconds(10));
        Require(!denied.Saved&&!denied.Retryable,"A permission denial was retried.");
        Task<CaptureUploadResult>? cancelled=null;
        var sentBeforeCancellation=fixture.Handler.Bodies.Count;
        await fixture.OnWorker(()=>{cancelled=worker.Upload("1",capture);worker.CancelQueuedCaptures();});
        Require(!(await cancelled!.WaitAsync(TimeSpan.FromSeconds(10))).Retryable&&fixture.Handler.Bodies.Count==sentBeforeCancellation,"A cancelled visit sent a queued capture.");
    }
    internal static async Task RetainerListings()
    {
        using var fixture=new Fixture();await fixture.Connect();
        var worker=fixture.Worker;var now=DateTimeOffset.UtcNow;
        fixture.Handler.ReadRoster=true;
        fixture.Handler.RegisteredCharacters=worker.View.Characters;
        fixture.Handler.Snapshots=[new("1",1,now,now,[new(0,8,false,5,100),new(1,8,false,5,100)])];
        Require(worker.RefreshRetainers(),"Retainer refresh was not queued.");
        await Until(()=>worker.View.RetainerSnapshots?.SingleOrDefault()?.Listings.Length==2&&!worker.View.RefreshingRetainers);
        Require(worker.View.RetainerSnapshots!.Single().Listings.Select(l=>l.Slot).SequenceEqual(new byte[]{0,1}),"Refreshing collapsed identical stacks.");
        await fixture.OnWorker(()=>{
            Require(worker.RefreshRetainers(),"Bound retainer refresh was not queued.");
            var other=fixture.Session with{Id="replacement",AccountId="456",Scopes=[]};
            fixture.SetSession(other);Set(worker,"loadedDevice",other.Id);
        });
        await fixture.OnWorker(()=>{});
        Require(worker.View.RetainerSnapshots is null,"A replacement account could see saved retainer listings.");
        Console.WriteLine("PASS: stable retainer listings, separate identical stacks, acknowledged price updates, revision ordering and account-bound refresh.");
    }
    internal static async Task Notifications()
    {
        using var fixture=new Fixture();await fixture.Connect();
        var worker=fixture.Worker;var now=DateTimeOffset.UtcNow;
        var path=Path.Combine(fixture.Root,"notifications.json");
        var history=new NotificationHistory(path);
        var alert=new GameDelivery("1",42,"alert","Price alert","Fixture",new("none",null,null),now.AddHours(1));
        var test=alert with{Id="2",EventId=null,Kind="test"};history.Receive([alert,test],now);
        var session=fixture.Session with{Scopes=["notifications:mark-read"]};
        await fixture.OnWorker(()=>{fixture.SetSession(session);Set(worker,"history",history);});
        await Until(()=>worker.View.Inbox.Length==2);
        Require(worker.MarkRead(alert.Id),"Read action was not queued.");
        await Until(()=>worker.View.ReadNotificationIds?.Contains(alert.Id)==true);
        using(var body=JsonDocument.Parse(fixture.Handler.NotificationReads.Single()))
            Require(body.RootElement.GetProperty("ids")[0].GetString()==alert.Id,"Mark read sent an event ID instead of a delivery ID.");
        Require(worker.MarkRead(test.Id),"Test read action was not queued.");
        await Until(()=>worker.View.ReadNotificationIds?.Contains(test.Id)==true);
        Require(fixture.Handler.NotificationReads.Count==1,"A local test notification changed website history.");
        var queue=(ConcurrentQueue<GameDelivery>)typeof(ClientWorker).GetField("toasts",BindingFlags.Instance|BindingFlags.NonPublic)!.GetValue(worker)!;
        queue.Enqueue(alert);
        Require(worker.DeleteNotifications(worker.View.Session,[alert.Id]),"Delete action was not queued.");
        await Until(()=>worker.View.Inbox.Length==1);
        Require(!worker.TryToast(out _),"A deleted notification still appeared in game chat.");
        Require(new NotificationHistory(path).IsDeleted(alert.Id),"Worker deletion was not durable.");
        await fixture.OnWorker(()=>{
            Require(worker.DeleteNotifications(session,[test.Id]),"Bound deletion was not queued.");
            Require(worker.MarkRead(test.Id),"Bound read action was not queued.");
            var replacement=session with{Id="replacement",AccountId="456"};fixture.SetSession(replacement);Set(worker,"loadedDevice",replacement.Id);
        });
        await fixture.OnWorker(()=>{});
        Require(history.Entries.Length==1&&fixture.Handler.NotificationReads.Count==1,"A queued history action crossed accounts.");
        Console.WriteLine("PASS: worker read/delete lifecycle, pending chat suppression, durability and account-bound history actions.");
    }
    internal static async Task CartLifecycle()
    {
        using var fixture=new Fixture();await fixture.Connect();
        using var server=Document.Open();server.Edit(new{kind="add",item_id=8,quality="nq",need=10});
        var path=Path.Combine(fixture.Connection.Directory,"accounts","123","lists","1.json");
        ConnectedCart? cart=null;
        async Task Dirty(bool keepBlocked)
        {
            await fixture.OnWorker(()=>{
                cart=new(fixture.Connection.Directory,"https://ultros.app/","123",1);
                if(!cart.Document.View.Ready){cart.Receive(new(server.View.Version,"snapshot",Convert.ToBase64String(server.Export())));cart.Save();}
                File.Move(path,path+".fixture-saved");Directory.CreateDirectory(path);
                try{cart.Edit(new{kind="add_acquired",item_id=8,quality="nq",delta=1});}
                catch(Exception e)when(e is IOException or UnauthorizedAccessException){}
                Require(cart.Document.View.AwaitingPersist,"Fixture did not retain a failed save.");
                if(!keepBlocked){Directory.Delete(path);File.Move(path+".fixture-saved",path);}
                Set(fixture.Worker,"cart",cart);
            });
            await Until(()=>fixture.Worker.View.Cart?.AwaitingPersist==true);
        }
        await Dirty(false);
        Require(fixture.Worker.Disconnect(),"Disconnect was not queued.");
        await Until(()=>!fixture.Worker.View.Connected&&fixture.Worker.View.Cart is null);
        using(var reopened=new ConnectedCart(fixture.Connection.Directory,"https://ultros.app/","123",1))
            Require(reopened.Document.View.Rows.Single().Acquired==1,"Disconnect lost the edit after storage recovered.");

        await fixture.OnWorker(()=>{fixture.SetSession(fixture.Session);Set(fixture.Worker,"loadedDevice",fixture.Session.Id);});
        await Until(()=>fixture.Worker.View.Connected);
        await Dirty(true);
        Require(fixture.Worker.Disconnect(),"Blocked disconnect was not queued.");
        await Until(()=>fixture.Worker.View.Status.StartsWith("Disconnect paused",StringComparison.Ordinal));
        Require(fixture.Worker.View.Connected&&fixture.Worker.View.Cart?.AwaitingPersist==true,"A persistent save failure discarded the document or connection.");
        await fixture.OnWorker(()=>{Directory.Delete(path);File.Move(path+".fixture-saved",path);});
        Require(fixture.Worker.Disconnect(),"Disconnect retry was not queued.");
        await Until(()=>!fixture.Worker.View.Connected);
        using(var reopened=new ConnectedCart(fixture.Connection.Directory,"https://ultros.app/","123",1))
            Require(reopened.Document.View.Rows.Single().Acquired==2,"Disconnect retry did not save exactly once.");

        await fixture.OnWorker(()=>{fixture.SetSession(fixture.Session);Set(fixture.Worker,"loadedDevice",fixture.Session.Id);});
        await Until(()=>fixture.Worker.View.Connected);
        await Dirty(true);
        var oldToasts=(ConcurrentQueue<GameDelivery>)typeof(ClientWorker).GetField("toasts",BindingFlags.Instance|BindingFlags.NonPublic)!.GetValue(fixture.Worker)!;
        await fixture.OnWorker(()=>{
            oldToasts.Enqueue(new("1",null,"test","Old account","Private message",new("none",null,null),DateTimeOffset.UtcNow.AddMinutes(1)));
            fixture.SetSession(fixture.Session with{Id="new-device",AccountId="456",Scopes=[]});
        });
        await Until(()=>fixture.Worker.View.Session?.Id=="new-device"&&fixture.Worker.View.Cart is null);
        await Until(()=>oldToasts.IsEmpty);
        Require(fixture.Worker.View.Lists.Length==0&&fixture.Worker.View.Characters.Length==0&&!fixture.Worker.View.CanEdit,"A new account saw or could edit the retained old document.");
        await fixture.OnWorker(()=>{Directory.Delete(path);File.Move(path+".fixture-saved",path);});
        await Until(()=>!cart!.Document.View.AwaitingPersist);
        using(var reopened=new ConnectedCart(fixture.Connection.Directory,"https://ultros.app/","123",1))
            Require(reopened.Document.View.Rows.Single().Acquired==3,"Account replacement lost the old account's unsaved edit.");
        Console.WriteLine("PASS: acknowledged/retryable captures and cart preservation across disconnect, persistent save failure and account replacement.");
    }
}
