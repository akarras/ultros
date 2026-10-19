using Dalamud.Game.Text;
using Dalamud.Game.Text.SeStringHandling;
using Dalamud.Interface.ImGuiNotification;
using Dalamud.Plugin.Services;
using FFXIVClientStructs.FFXIV.Client.UI;
using Ultros.Client;

namespace UltrosPlugin;

internal sealed class GameNotifications : IDisposable
{
    private readonly ClientWorker worker;
    private readonly Configuration config;
    private readonly RetainerCapture capture;
    private DateTimeOffset nextSound;
    private readonly Queue<string> awaitingAck=[];
    internal static readonly XivChatType[] ChatTypes = [XivChatType.Echo, XivChatType.SystemMessage, XivChatType.Notice];
    internal static readonly string[] ChatLabels = ["Echo", "System messages", "Notices"];
    internal GameNotifications(ClientWorker worker, Configuration config, RetainerCapture capture)
    {
        this.worker=worker;this.config=config;this.capture=capture;
        Plugin.Framework.Update+=Update;
    }
    private static string Plain(string value)=>new(value.Where(c=>!char.IsControl(c)).Take(2000).ToArray());
    private void Print(string title,string body)
    {
        var type=ChatTypes.FirstOrDefault(t=>(int)t==config.NotificationChatType,XivChatType.Echo);
        Plugin.Chat.Print(new XivChatEntry{Type=type,Message=new SeStringBuilder().AddUiForeground("[Ultros] ",45).AddText($"{Plain(title)} — {Plain(body)}").Build()});
        if(config.NotificationPopup)
        {
            try{Plugin.Notifications.AddNotification(new Notification{Title=Plain(title),Content=Plain(body)});}
            catch(Exception error){Plugin.Log.Warning("Ultros popup failed ({Type}); the message was printed in chat.",error.GetType().Name);}
        }
    }
    internal unsafe void PlaySound()
    {
        if(capture.Compatible&&Plugin.Player.IsLoaded)
            UIGlobals.PlayChatSoundEffect((uint)Math.Clamp(config.NotificationSoundId,1,16));
    }
    internal void Test()
    {
        Print("Notification test","Ultros notifications appear here. Choose a chat category and optional sound in Settings.");
        if(config.NotificationSound)PlaySound();
    }
    private void Update(IFramework framework)
    {
        if(!Plugin.Player.IsLoaded)return;
        while(awaitingAck.TryPeek(out var id)&&worker.ToastShown(id))awaitingAck.Dequeue();
        if(awaitingAck.Count>=1000)return;
        for(var i=0;i<10&&worker.TryToast(out var delivery);i++)
        {
            if(delivery is null||delivery.ExpiresAt<=DateTimeOffset.UtcNow)continue;
            try{Print(delivery.Title,delivery.Body);awaitingAck.Enqueue(delivery.Id);}
            catch(Exception error){Plugin.Log.Warning("Ultros notification could not be displayed ({Type}); history is preserved.",error.GetType().Name);continue;}
            if(config.NotificationSound&&DateTimeOffset.UtcNow>=nextSound)
            {
                nextSound=DateTimeOffset.UtcNow.AddSeconds(3);
                try{PlaySound();}catch(Exception error){Plugin.Log.Warning("Ultros notification sound failed ({Type}).",error.GetType().Name);}
            }
        }
    }
    public void Dispose()=>Plugin.Framework.Update-=Update;
}
