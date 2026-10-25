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
        internal HttpStatusCode? Failure=HttpStatusCode.ServiceUnavailable;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellation)
        {
            if(request.Method==HttpMethod.Get&&request.RequestUri!.AbsolutePath=="/api/v1/plugin/lists")
                return new(HttpStatusCode.OK){Content=new StringContent("[]")};
            if(request.Method==HttpMethod.Get&&request.RequestUri!.AbsolutePath=="/api/v1/plugin/characters")
                return new(HttpStatusCode.ServiceUnavailable){Content=new StringContent("{}")};
            Require(request.RequestUri!.AbsolutePath=="/api/v1/plugin/retainers/1/listings","Unexpected fixture request.");
            Bodies.Add(await request.Content!.ReadAsStringAsync(cancellation));
            return new(Failure??HttpStatusCode.OK){Content=new StringContent(Failure is null?"{\"revision\":1,\"hash\":\"fixture\"}":"{\"error\":\"service_unavailable\"}",Encoding.UTF8,"application/json")};
        }
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
        await fixture.OnWorker(()=>Set(worker,"characters",worker.View.Characters.Select(c=>c with{Retainers=c.Retainers.Select(r=>r with{Revision=2}).ToArray()}).ToArray()));
        Require((await worker.Upload("1",capture)!.WaitAsync(TimeSpan.FromSeconds(10))).Saved
            &&worker.View.Characters.Single().Retainers.Single().Revision==2,"An old duplicate receipt downgraded a newer known revision.");
        fixture.Handler.Failure=HttpStatusCode.Forbidden;
        var denied=await worker.Upload("1",capture)!.WaitAsync(TimeSpan.FromSeconds(10));
        Require(!denied.Saved&&!denied.Retryable,"A permission denial was retried.");
        Task<CaptureUploadResult>? cancelled=null;
        var sentBeforeCancellation=fixture.Handler.Bodies.Count;
        await fixture.OnWorker(()=>{cancelled=worker.Upload("1",capture);worker.CancelQueuedCaptures();});
        Require(!(await cancelled!.WaitAsync(TimeSpan.FromSeconds(10))).Retryable&&fixture.Handler.Bodies.Count==sentBeforeCancellation,"A cancelled visit sent a queued capture.");
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
