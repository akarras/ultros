using Dalamud.Game.Command;
using Dalamud.Interface.ImGuiNotification;
using Dalamud.Interface.Windowing;
using Dalamud.IoC;
using Dalamud.Plugin;
using Dalamud.Plugin.Services;
using Ultros.Lists;
using Ultros.Client;

namespace UltrosPlugin;

public sealed class Plugin : IDalamudPlugin
{
    [PluginService] internal static IDalamudPluginInterface Interface { get; private set; } = null!;
    [PluginService] internal static ICommandManager Commands { get; private set; } = null!;
    [PluginService] internal static IDataManager Data { get; private set; } = null!;
    [PluginService] internal static INotificationManager Notifications { get; private set; } = null!;
    [PluginService] internal static IChatGui Chat { get; private set; } = null!;
    [PluginService] internal static IContextMenu ContextMenu { get; private set; } = null!;
    [PluginService] internal static IPluginLog Log { get; private set; } = null!;
    private readonly WindowSystem windows = new("Ultros");
    [PluginService] internal static IFramework Framework { get; private set; } = null!;
    [PluginService] internal static IPlayerState Player { get; private set; } = null!;
    [PluginService] internal static IGameGui GameGui { get; private set; } = null!;
    [PluginService] internal static IAddonLifecycle AddonLifecycle { get; private set; } = null!;
    [PluginService] internal static ICondition Condition { get; private set; } = null!;
    [PluginService] internal static IGameInventory Inventory { get; private set; } = null!;
    private readonly ClientWorker worker;
    private readonly RetainerCapture capture;
    private readonly InventoryAcquisition inventoryProgress;
    private readonly MainWindow window;
    private readonly GameNotifications gameNotifications;
    private readonly ItemShortcuts itemShortcuts;
    private MergeLabWindow? lab;
    private static Action? openLab;

    public Plugin()
    {
        if(Commands.Commands.ContainsKey("/ultros"))
            throw new InvalidOperationException("Another copy of Ultros is already loaded. Disable the older Ultros entry before enabling this installation.");
        Native.Configure(Interface.AssemblyLocation.Directory!.FullName,Path.Combine(Interface.GetPluginConfigDirectory(),"native"));
        var config=Interface.GetPluginConfig() as Configuration??new Configuration();
        if(!UltrosEndpoint.LocalhostAvailable&&config.Localhost){config.Localhost=false;Interface.SavePluginConfig(config);}
        var endpoint=new UltrosEndpoint(config.Localhost,config.LocalOrigin);
        worker=new ClientWorker(Path.Combine(Interface.GetPluginConfigDirectory(),"connected"),endpoint,config.Profile);
        capture=new RetainerCapture(worker,config);
        inventoryProgress=new InventoryAcquisition(worker,capture,config);
        window=new MainWindow(worker,capture,inventoryProgress,config);
        var retainerPrices=new RetainerPricesWindow(worker,window);
        window.OpenRetainerPrices=retainerPrices.Show;
        gameNotifications=new GameNotifications(worker,config,capture);
        itemShortcuts=new ItemShortcuts(worker,window,endpoint);
        testNotifications=gameNotifications.Test;
        windows.AddWindow(window);
        windows.AddWindow(retainerPrices);
        openLab=ShowLab;
        Commands.AddHandler("/ultros", new CommandInfo(OnCommand) { HelpMessage = "Open Ultros: connect, shop, register retainers and receive alerts." });
        if(UltrosEndpoint.LocalhostAvailable)Commands.AddHandler("/ultroslab",new CommandInfo((_,_)=>ShowLab()){HelpMessage="Open the local CRDT merge experiment."});
        Interface.UiBuilder.Draw += windows.Draw;
        Interface.UiBuilder.OpenMainUi += Toggle;
        Interface.UiBuilder.OpenConfigUi += Toggle;
    }

    private void OnCommand(string command, string arguments) => Toggle();
    private void Toggle() => window.Toggle();
    internal static void OpenLab()=>openLab?.Invoke();
    private void ShowLab()
    {
        if(lab is null)
        {
            var store=new LocalCartStore(Path.Combine(Interface.GetPluginConfigDirectory(),"merge-lab-carts"));
            string? error=null;
            try{store.OpenProfile("main");}catch{error="Local cart could not open. Choose an unused profile; existing files are preserved.";}
            lab=new MergeLabWindow(store,error);windows.AddWindow(lab);
        }
        lab.IsOpen=true;
    }
    private static Action? testNotifications;
    internal static void TestNotification()=>testNotifications?.Invoke();

    public void Dispose()
    {
        Interface.UiBuilder.Draw -= windows.Draw;
        Interface.UiBuilder.OpenMainUi -= Toggle;
        Interface.UiBuilder.OpenConfigUi -= Toggle;
        Commands.RemoveHandler("/ultros");if(UltrosEndpoint.LocalhostAvailable)Commands.RemoveHandler("/ultroslab");
        itemShortcuts.Dispose();gameNotifications.Dispose();inventoryProgress.Dispose();capture.Dispose();worker.Dispose();openLab=null;testNotifications=null;
        windows.RemoveAllWindows();
        lab?.Dispose();
    }
}
