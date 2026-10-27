using System.Text.Json;
using Ultros.Client;

namespace Ultros.Lists.Smoke;

internal static class HistoryPrivacyChecks
{
    private static void Require(bool ok,string message){if(!ok)throw new Exception(message);}
    internal static void Run()
    {
        var now=DateTimeOffset.UtcNow;
        var session=new DeviceSession("device","Game client","123456789012345678","streamer",[],now,now,now.AddDays(1),1);
        var privacy=new AccountPrivacy();
        privacy.Track(session,true,true);Require(!privacy.Revealed,"Account started revealed.");
        privacy.Revealed=true;privacy.Track(session,true,true);Require(privacy.Revealed,"Reveal did not survive the current visit.");
        privacy.Track(session,false,true);Require(!privacy.Revealed,"Leaving Account exposed the next visit.");
        privacy.Revealed=true;privacy.Track(session with{Id="replacement"},true,true);Require(!privacy.Revealed,"Reauthorization kept identity revealed.");
        privacy.Revealed=true;privacy.Track(session,true,false);Require(!privacy.Revealed,"Disconnect kept identity revealed.");
        privacy.Revealed=true;privacy.Reset();Require(!privacy.Revealed,"Closing the window kept identity revealed.");
        foreach(var name in new[]{"a","ab","chewbert","👩🏽‍💻x","e\u0301x"})
            Require(AccountPrivacy.MaskName(name)!=name&&!AccountPrivacy.MaskName(name).Contains(session.AccountId),"Mask exposed an entire username.");
        var dir=Path.Combine(Path.GetTempPath(),"ultros-history-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            var path=Path.Combine(dir,"history.json");
            var a=new GameDelivery("1",23,"alert","Price changed","An item is cheaper.",new("none",null,null),now.AddHours(1));
            var b=a with{Id="2",EventId=null,Kind="test"};
            // Existing v1 files must migrate without replaying already shown notifications.
            File.WriteAllBytes(path,JsonSerializer.SerializeToUtf8Bytes(new{entries=new[]{a,b},shown=new[]{a.Id}},Protocol.Json));
            var history=new NotificationHistory(path);
            Require(history.WasShown(a.Id)&&history.Entries.Length==2,"Old history was lost.");
            history.MarkRead(a.Id);history.Delete([a.Id],now);
            history=new NotificationHistory(path);
            history.Receive([a,b],now);
            Require(history.Entries.Select(d=>d.Id).SequenceEqual([b.Id])&&history.IsDeleted(a.Id),"Deleted delivery returned after reload/retry.");
            Require(history.ReadIds.Length==0,"Deleted history kept stale read markers.");
            var clearing=history.Entries.Select(d=>d.Id).ToArray();
            var fresh=a with{Id="3"};history.Receive([fresh],now);history.Delete(clearing,now);
            Require(history.Entries.Select(d=>d.Id).SequenceEqual([fresh.Id]),"Clear removed a newly arrived alert.");
            Require(new NotificationHistory(Path.Combine(dir,"other-device.json")).Entries.Length==0,"History crossed devices.");
            // A failed checkpoint must leave the UI entry and suppression state unchanged.
            File.Delete(path);Directory.CreateDirectory(path);
            try{history.Delete([fresh.Id],now);throw new Exception("Failed disk write was accepted.");}
            catch(IOException){}
            catch(UnauthorizedAccessException){}
            Require(history.Entries.Length==1&&!history.IsDeleted(fresh.Id),"Failed deletion changed live state.");
        }
        finally{Directory.Delete(dir,true);}
        Console.WriteLine("Notification history durability and account privacy checks passed.");
    }
}
