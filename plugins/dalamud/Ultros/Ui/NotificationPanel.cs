using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed class NotificationPanel
{
    internal Action Test { get; init; }=()=>{};
    internal Action Refresh { get; init; }=()=>{};
    internal Action<string> MarkRead { get; init; }=_=>{};
    internal Action<DeviceSession?,string[]> Delete { get; init; }=(_,_)=>{};
    internal Action<int> OpenItem { get; init; }=_=>{};
    internal Action<int> OpenList { get; init; }=_=>{};
    private DeviceSession? clearingSession;
    private string[] clearing=[];
    internal void Draw(ClientView v)
    {
        UiKit.Heading("Notification history","Alerts received by this game client. Deleting history keeps your alert rules.");
        ImGui.BeginDisabled(!v.Connected);
        if(ImGui.Button("Send test alert"))Test();
        UiKit.Inline(120);if(ImGui.Button("Refresh"))Refresh();
        ImGui.EndDisabled();
        if(v.Inbox.Length>0)
        {
            UiKit.Inline(160);
            if(ImGui.Button("Clear history...")){clearing=v.Inbox.Select(d=>d.Id).ToArray();clearingSession=v.Session;ImGui.OpenPopup("Clear notification history?");}
        }
        if(ImGui.BeginPopupModal("Clear notification history?",ImGuiWindowFlags.AlwaysAutoResize))
        {
            ImGui.TextUnformatted($"Remove {clearing.Length} notifications from this client's history?");
            ImGui.TextUnformatted("New alerts and your alert rules are kept.");
            if(v.Session?.Id!=clearingSession?.Id){clearing=[];ImGui.CloseCurrentPopup();}
            else if(ImGui.Button("Clear history")){Delete(clearingSession,clearing);ImGui.CloseCurrentPopup();}
            ImGui.SameLine();if(ImGui.Button("Cancel"))ImGui.CloseCurrentPopup();
            ImGui.EndPopup();
        }
        UiKit.Hint($"{v.Inbox.Length} saved · {v.NotificationTransport}");
        if(v.Inbox.Length==0)UiKit.Empty("You're all caught up",v.Connected?"New alerts will appear here and in game chat. Send a test alert to try it.":"Connect your account to receive alerts here.");
        foreach(var entry in v.Inbox.Reverse())
        {
            ImGui.PushID(entry.Id);
            try
            {
                UiKit.Section(entry.Title);
                if(v.ReadNotificationIds?.Contains(entry.Id)==true)UiKit.Muted("Read");
                else if(entry.Kind=="test")UiKit.Muted("Test alert");
                ImGui.TextWrapped(entry.Body);ImGui.Spacing();
                if(entry.Action.Kind=="open_item"&&entry.Action.ItemId is { } item)
                {if(ImGui.Button("View item"))OpenItem(item);UiKit.Inline(120);}
                if(entry.Action.Kind=="open_list"&&entry.Action.ListId is { } list)
                {if(ImGui.Button("Open list"))OpenList(list);UiKit.Inline(120);}
                ImGui.BeginDisabled(v.ReadNotificationIds?.Contains(entry.Id)==true||!v.Connected||!v.Scopes.Contains("notifications:mark-read"));
                if(ImGui.Button("Mark read"))MarkRead(entry.Id);
                ImGui.EndDisabled();
                UiKit.Inline(90);if(ImGui.Button("Delete"))Delete(v.Session,[entry.Id]);
            }finally{ImGui.PopID();}
        }
    }
}
