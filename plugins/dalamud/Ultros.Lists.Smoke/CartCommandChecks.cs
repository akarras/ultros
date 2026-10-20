using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class CartCommandChecks
{
    private static void Require(bool condition,string message) { if(!condition)throw new Exception(message); }
    internal static void Run()
    {
        var root=Path.Combine(Path.GetTempPath(),"ultros-cart-commands-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            var now=DateTimeOffset.UtcNow;
            var session=new DeviceSession("device","Fixture","123","Fixture",["lists:read","lists:write"],now,now,now.AddDays(1),null);
            using var server=Document.Open();
            server.Edit(new{kind="add",item_id=8,quality="nq",need=10});
            SyncReply Snapshot(Document doc)=>new(doc.View.Version,"snapshot",Convert.ToBase64String(doc.Export()));
            ConnectedCart Open(int id) {
                var cart=new ConnectedCart(root,"https://ultros.app/",session.AccountId,id);
                if(!cart.Document.View.Ready){cart.Receive(Snapshot(server));cart.Save();}
                return cart;
            }
            ClientView View(ConnectedCart cart)=>new(true,"","",null,"https://ultros.app/plugin/verify",session,[],cart.Id,cart.Document.View,true,[],[],null,[],null,[],CartInstance:cart.InstanceId,RecoveryRevision:cart.RecoveryRevision);
            using var a=Open(1);
            using var b=Open(2);
            var destination=CartCommandTarget.Capture(View(a))!;
            ConnectedCart current=a;
            var queued=new Queue<Action>();
            queued.Enqueue(()=>current=b);
            queued.Enqueue(()=>Require(!CartCommands.Edit(destination,session,current,true,new{kind="add_acquired",item_id=8,quality="nq",delta=1}),"Old A edit applied after selecting B."));
            queued.Enqueue(()=>Require(!CartCommands.Edit(destination,session,current,true,new{kind="undo"}),"Old A undo applied after selecting B."));
            while(queued.TryDequeue(out var command))command();
            Require(a.Document.View.Rows.Single().Acquired==0&&b.Document.View.Rows.Single().Acquired==0,"Rejected commands changed a cart.");
            for(var i=0;i<2;i++)Require(CartCommands.Edit(destination,session,a,true,new{kind="add_acquired",item_id=8,quality="nq",delta=1}),"Consecutive edits from one displayed version were dropped.");
            Require(a.Document.View.Rows.Single().Acquired==2,"Two queued gains did not both count.");
            Require(!CartCommands.Edit(destination,session with{AccountId="456"},a,true,new{kind="undo"}),"Another account accepted an old command.");
            Require(!CartCommands.Edit(destination,session with{Id="other"},a,true,new{kind="undo"}),"Another device accepted an old command.");
            Require(!CartCommands.Edit(destination,session,a,false,new{kind="undo"}),"Revoked write permission accepted an edit.");
            a.Dispose();
            using var reopened=Open(1);
            Require(!CartCommands.Edit(destination,session,reopened,true,new{kind="undo"}),"A→B→A reopening accepted an old instance command.");

            using var remote1=Document.Open();remote1.Edit(new{kind="add",item_id=8,quality="nq",need=20});
            reopened.Receive(Snapshot(remote1));reopened.Save();
            Require(reopened.Document.View.RecoveryReview,"Unrelated documents did not enter recovery review.");
            var first=RecoveryChoiceTarget.Capture(View(reopened))!;
            Require(CartCommands.Recover(first,session,reopened,false),"Current retry choice was rejected.");
            Require(!CartCommands.Recover(first,session,reopened,false),"One retry choice was consumed twice.");
            using var remote2=Document.Open();remote2.Edit(new{kind="add",item_id=8,quality="nq",need=30});
            reopened.Receive(Snapshot(remote2));reopened.Save();
            Require(reopened.Document.View.RecoveryReview&&reopened.Document.View.Version==first.Version,"Fixture must replace the recovery candidate without changing the local version.");
            var path=Path.Combine(root,"accounts",session.AccountId,"lists","1.json");
            var checkpoint=File.ReadAllBytes(path);var backup=File.ReadAllBytes(path+".recovery-original");
            Require(!CartCommands.Recover(first,session,reopened,true),"Old server-copy choice applied to a new recovery candidate.");
            Require(checkpoint.SequenceEqual(File.ReadAllBytes(path))&&backup.SequenceEqual(File.ReadAllBytes(path+".recovery-original")),"Rejected recovery touched its checkpoint or backup.");
            var second=RecoveryChoiceTarget.Capture(View(reopened))!;
            Require(!CartCommands.Recover(second,session,b,true),"Recovery choice applied to a different cart.");
            Require(CartCommands.Recover(second,session,reopened,true),"Current server-copy choice was rejected.");
            Require(reopened.Document.View.Rows.Single().Need==30,"The selected server candidate was not used.");
            Require(!CartCommands.Recover(second,session,reopened,true),"One server-copy choice was consumed twice.");
        }
        finally { Directory.Delete(root,true); }
    }
}
