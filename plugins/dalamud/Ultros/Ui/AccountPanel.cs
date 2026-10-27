using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed record ConnectionOptions(string Label,bool EditLists,bool Register,bool Upload,bool ReadRetainers);
internal sealed class AccountPanel
{
    private readonly AccountPrivacy privacy=new();
    private string label="My game client";
    private bool editLists=true,register=true,upload=true,readRetainers=true,revealCode;
    private string? pendingCode;
    internal Action<ConnectionOptions> Connect { get; init; }=_=>{};
    internal Action Disconnect { get; init; }=()=>{};
    internal Action Refresh { get; init; }=()=>{};
    internal Action CancelPairing { get; init; }=()=>{};
    internal Action<string> Open { get; init; }=_=>{};
    internal void Track(ClientView view,bool selected)
    {
        privacy.Track(view.Session,selected,view.Connected);
        if(!selected||pendingCode!=view.HumanCode)revealCode=false;
        pendingCode=view.HumanCode;
    }
    internal void Reset(){privacy.Reset();revealCode=false;}
    internal void Draw(ClientView v)
    {
        UiKit.Heading("Account",v.Connected?"This game client is connected to Ultros.":v.Session is not null?"Your saved connection needs attention.":"Connect your game client to sync with Ultros.");
        if(v.Session is { } session)
        {
            UiKit.Muted(v.Connected?"CONNECTED ACCOUNT":"SAVED ACCOUNT");
            ImGui.TextUnformatted(privacy.Revealed?session.AccountName:AccountPrivacy.MaskName(session.AccountName));
            if(privacy.Revealed)ImGui.TextUnformatted($"Account ID: {session.AccountId}");
            var reveal=privacy.Revealed;
            ImGui.BeginDisabled(!v.Connected);
            if(ImGui.Checkbox("Reveal account details for this visit",ref reveal))privacy.Revealed=reveal;
            ImGui.EndDisabled();
            UiKit.Hint("Hidden again when you leave Account or close this window.");
            ImGui.Spacing();ImGui.TextUnformatted($"Game client: {session.Label}");
            if(!v.Connected&&ImGui.Button("Retry connection"))Refresh();
            if(ImGui.Button("Manage connected clients"))Open(new Uri(new Uri(v.VerificationUrl),"/plugin/devices").AbsoluteUri);
            UiKit.Inline();if(ImGui.Button("Disconnect..."))ImGui.OpenPopup("Disconnect this game client?");
            if(ImGui.BeginPopupModal("Disconnect this game client?",ImGuiWindowFlags.AlwaysAutoResize))
            {
                ImGui.TextUnformatted("List sync, retainer uploads and alerts will stop on this client.");
                ImGui.TextUnformatted("Your saved lists and retainer listings stay on Ultros.");
                if(ImGui.Button("Disconnect")){Disconnect();Reset();ImGui.CloseCurrentPopup();}
                ImGui.SameLine();if(ImGui.Button("Cancel"))ImGui.CloseCurrentPopup();
                ImGui.EndPopup();
            }
            UiKit.Section("Connection Permissions");
            if(ImGui.BeginTable("feature-access",2,ImGuiTableFlags.SizingStretchProp))
            {
                ImGui.TableSetupColumn("Feature",ImGuiTableColumnFlags.WidthStretch,1);
                ImGui.TableSetupColumn("Access",ImGuiTableColumnFlags.WidthFixed,90*UiKit.Scale);
                foreach(var (scope,title) in new[]{("lists:write","Edit and sync lists"),("retainers:read","View your retainer listings"),("retainers:register","Save observed retainers"),("retainers:upload","Upload selling prices"),("notifications:read","Receive game alerts")})
                {
                    ImGui.TableNextRow();ImGui.TableNextColumn();ImGui.TextUnformatted(title);
                    ImGui.TableNextColumn();UiKit.Muted(v.Scopes.Contains(scope)?"Allowed":"Not approved");
                }
                ImGui.EndTable();
            }
            if(v.HumanCode is null&&ImGui.CollapsingHeader("Change connection permissions"))ConnectionForm(true);
        }
        else if(v.HumanCode is null)
        {
            ImGui.TextUnformatted("Lists, retainer prices and alerts in game.");
            UiKit.Hint("Approve this client in your browser using your Ultros account.");
            ImGui.Spacing();ConnectionForm(false);
        }
        if(v.HumanCode is { } code)
        {
            UiKit.Section("Waiting for browser approval");
            UiKit.Hint("Open the approval page, check the account and approve this game client.");
            if(ImGui.Button("Open approval page"))Open(v.ApprovalUrl??v.VerificationUrl);
            UiKit.Inline();if(ImGui.Button("Cancel request")){CancelPairing();revealCode=false;}
            if(ImGui.CollapsingHeader("Use a phone or another browser"))
            {
                ImGui.TextWrapped(v.VerificationUrl);
                ImGui.TextUnformatted(revealCode?$"Game code: {code}":"Game code: ****-****");
                ImGui.Checkbox("Reveal game code",ref revealCode);
                if(ImGui.Button("Copy game code"))ImGui.SetClipboardText(code);
            }
        }
        if(ImGui.CollapsingHeader("Connection details")){UiKit.Hint(v.Status);UiKit.Hint(v.StorageMode);}
    }
    private void ConnectionForm(bool connected)
    {
        ImGui.TextUnformatted("Game client name");
        ImGui.SetNextItemWidth(-1);ImGui.InputTextWithHint("##client-label","Name this game client",ref label,64);
        if(ImGui.CollapsingHeader("Customize permissions"))
        {
            ImGui.Checkbox("Edit and sync lists",ref editLists);
            if(ImGui.Checkbox("View retainer listings",ref readRetainers)&&!readRetainers)register=upload=false;
            if(ImGui.Checkbox("Save observed retainers",ref register)&&register)readRetainers=true;
            if(ImGui.Checkbox("Upload private selling prices",ref upload)&&upload)readRetainers=true;
        }
        if(ImGui.Button(connected?"Review permissions in browser":"Connect to Ultros"))Connect(new(label,editLists,register,upload,readRetainers));
    }
}
