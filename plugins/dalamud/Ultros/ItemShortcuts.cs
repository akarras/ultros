using Dalamud.Game.Gui.ContextMenu;
using Dalamud.Plugin.Services;
using Dalamud.Utility;
using Ultros.Client;

namespace UltrosPlugin;

internal sealed class ItemShortcuts : IDisposable
{
    private readonly ClientWorker worker;
    private readonly MainWindow window;
    private readonly UltrosEndpoint endpoint;
    private readonly Configuration config;
    private uint currentWorld,homeWorld;
    private DateTimeOffset retryWorld;
    internal ItemShortcuts(ClientWorker worker,MainWindow window,UltrosEndpoint endpoint,Configuration config)
    {
        this.worker=worker;this.window=window;this.endpoint=endpoint;this.config=config;
        Plugin.ContextMenu.OnMenuOpened+=OnMenuOpened;
        Plugin.Framework.Update+=Update;
    }
    private void Update(IFramework framework)
    {
        if(!Plugin.Player.IsLoaded)return;
        var current=Plugin.Player.CurrentWorld.RowId;
        var home=Plugin.Player.HomeWorld.RowId;
        if(current==currentWorld&&home==homeWorld&&(worker.View.PriceRegion.Length>0||DateTimeOffset.UtcNow<retryWorld))return;
        if(worker.SetCurrentWorld((int)current,(int)home)){currentWorld=current;homeWorld=home;retryWorld=DateTimeOffset.UtcNow.AddSeconds(15);}
    }
    private void OnMenuOpened(IMenuOpenedArgs args)
    {
        if(!config.ItemContextMenuEnabled)return;
        uint raw=0;
        if(args.Target is MenuTargetInventory inventory&&inventory.TargetItem is { } item)raw=item.ItemId;
        else
        {
            raw=(uint)Plugin.GameGui.HoveredItem;
            if(raw==0&&args.Target is MenuTargetDefault target&&(target.TargetObject is not null||target.TargetContentId!=0))return;
        }
        if(raw==0)raw=(uint)Plugin.GameGui.HoveredItem;
        if(raw==0&&args.AddonName?.StartsWith("ChatLog",StringComparison.Ordinal)==true)raw=Plugin.Chat.LastLinkedItemId;
        if(raw==0||ItemUtil.IsEventItem(raw))return;
        var id=ItemUtil.GetBaseId(raw).ItemId;
        if(!Plugin.Data.GetExcelSheet<Lumina.Excel.Sheets.Item>().TryGetRow(id,out var row))return;
        var view=worker.View;
        var destination=ListEditTarget.Capture(view);
        var listName=view.Cart?.Name??"active list";
        if(listName.Length>32)listName=listName[..32]+"…";
        args.AddMenuItem(new MenuItem{Name=destination is null?"Add to Ultros list…":$"Add to {listName}",PrefixChar='U',OnClicked=_=>{if(config.ItemContextMenuEnabled)window.AddToList((int)id,destination);}});
        if(row.ItemSearchCategory.RowId!=0)
            args.AddMenuItem(new MenuItem{Name="Search Ultros",PrefixChar='U',OnClicked=_=>{if(config.ItemContextMenuEnabled)window.ShowItem((int)id);}});
        args.AddMenuItem(new MenuItem{Name="Open Ultros website",PrefixChar='U',OnClicked=_=>
        {
            if(!config.ItemContextMenuEnabled)return;
            var region=worker.View.PriceRegion;
            Util.OpenLink(endpoint.At(region.Length>0?$"/item/{Uri.EscapeDataString(region)}/{id}":$"/item/{id}").AbsoluteUri);
        }});
    }
    public void Dispose(){Plugin.ContextMenu.OnMenuOpened-=OnMenuOpened;Plugin.Framework.Update-=Update;}
}
