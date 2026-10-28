using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed class BellRetainerPanel
{
    internal Action<ObservedCharacter> Refresh { get; init; }=_=>{};
    internal Action<ObservedCharacter> UseGameOrder { get; init; }=_=>{};
    internal Action Manage { get; init; }=()=>{};
    internal void Draw(ClientView view,ObservedCharacter character)
    {
        var missing=RetainerRoster.Missing(view.Characters,character);
        var allowed=view.Connected&&view.Scopes.Contains("retainers:register");
        ImGui.BeginDisabled(view.Registering);
        if(ImGui.SmallButton(view.Registering?"Ultros · Refreshing…":missing>0&&allowed?$"Ultros · Refresh retainers (+{missing})":"Ultros"))
        {
            if(missing>0&&allowed)Refresh(character);
            else ImGui.OpenPopup("Ultros retainers");
        }
        ImGui.EndDisabled();
        if(ImGui.IsItemHovered())ImGui.SetTooltip(missing>0?"Save missing retainers to your Ultros account.":"Refresh retainers, match the game's order, or manage disabled retainers.");
        ImGui.SetNextWindowBgAlpha(1);
        if(!ImGui.BeginPopup("Ultros retainers"))return;
        try {
            ImGui.TextUnformatted(character.Name);
            ImGui.Separator();
            if(ImGui.MenuItem("Refresh retainers","",false,allowed&&!view.Registering))Refresh(character);
            var saved=ClientWorker.LatestObservation(view.Characters,character);
            var canOrder=allowed&&view.RetainerPreferences&&!view.Registering&&saved is not null&&RetainerRoster.GameOrder(saved,character) is not null;
            if(ImGui.MenuItem("Use in-game order","",false,canOrder))UseGameOrder(character);
            if(ImGui.IsItemHovered())ImGui.SetTooltip("Save this character's retainer order on Ultros as well as in this client.");
            if(ImGui.MenuItem("Manage retainers…"))Manage();
            if(!allowed)UiKit.Hint("Approve retainer management in Account to refresh here.");
        }finally{ImGui.EndPopup();}
    }
}
