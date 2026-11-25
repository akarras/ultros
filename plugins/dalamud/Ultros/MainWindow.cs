using System.Numerics;
using Dalamud.Bindings.ImGui;
using Dalamud.Interface.Textures;
using Dalamud.Interface.Windowing;
using Ultros.Client;
using Ultros.Lists;
using UltrosPlugin.Ui;
using Item = Lumina.Excel.Sheets.Item;

namespace UltrosPlugin;

internal sealed class MainWindow : Window
{
    private readonly ClientWorker worker;
    private readonly RetainerCapture capture;
    private readonly InventoryAcquisition inventoryProgress;
    private readonly Configuration config;
    private readonly AccountPanel account;
    private readonly MarketPanel market;
    private readonly ListsPanel lists;
    private readonly NotificationPanel notifications;
    private readonly RetainerListingsPanel retainerListings;
    private readonly SettingsPanel settings;
    private Page page = Page.Account;
    private bool firstDraw = true;
    private bool styled;
    private bool openItem, openLists, openRetainers, manageRetainers;
    private string uiStatus = "";
    private string? browserError;

    internal MainWindow(ClientWorker worker, RetainerCapture capture, InventoryAcquisition inventoryProgress, Configuration config)
        : base($"Ultros v{ClientBuild.Version.ToString(4)}###UltrosConnected")
    {
        this.worker = worker;
        this.capture = capture;
        this.inventoryProgress = inventoryProgress;
        this.config = config;

        account = new AccountPanel
        {
            Connect = o => Submit(worker.Pair(o.Label, o.EditLists, o.Register, o.Upload, o.ReadRetainers)),
            Disconnect = () => Submit(worker.Disconnect()),
            CancelPairing = () => Submit(worker.CancelPairing()),
            Refresh = () => Submit(worker.Refresh()),
            Open = Open,
        };
        market = new MarketPanel
        {
            ItemName = ItemName,
            WorldName = WorldName,
            Marketable = Marketable,
            HasItem = HasItem,
            SearchItems = SearchMarketableItems,
            ItemIcon = ItemIcon,
            FetchItem = (id, s) => Submit(worker.FetchItem(id, s)),
            AddToList = (id, quality, qty) => AddToList(id, ListEditTarget.Capture(worker.View), quality, qty),
            OpenWebsite = OpenItemWebsite,
            Notify = msg => uiStatus = msg,
        };
        lists = new ListsPanel
        {
            ItemName = ItemName,
            WorldName = WorldName,
            HasItem = HasItem,
            SearchAllItems = SearchAllItems,
            ItemIcon = ItemIcon,
            RefreshAll = target => Submit(worker.RefreshListsAndEstimate(target)),
            SelectList = id => Submit(worker.SelectList(id)),
            RetrySave = target => Submit(worker.RetrySave(target)),
            RetryRecovery = target => Submit(worker.RetryRecovery(target)),
            UseServerCopy = target => Submit(worker.UseServerCopy(target)),
            AddItem = (dest, id, quality, qty) => Submit(worker.AddItem(dest, id, quality, qty)),
            Edit = (target, edit) => Submit(worker.Edit(target, edit)),
            EditBatch = (target, edits) => Submit(worker.EditBatch(target, edits)),
            OpenItem = id => ShowItem(id),
            Notify = msg => uiStatus = msg,
        };
        notifications = new NotificationPanel
        {
            Test = () => Submit(worker.TestNotification()),
            Refresh = () => Submit(worker.Refresh()),
            MarkRead = id => Submit(worker.MarkRead(id)),
            MarkAllRead = () => Submit(worker.MarkAllRead()),
            Delete = (session, ids) => Submit(worker.DeleteNotifications(session, ids)),
            OpenItem = id => ShowItem(id),
            OpenList = id => { Submit(worker.SelectList(id)); openLists = true; },
        };
        retainerListings = new RetainerListingsPanel
        {
            ItemName = ItemName,
            WorldName = WorldName,
            Marketable = Marketable,
            ItemIcon = ItemIcon,
            OpenItem = id => ShowItem(id),
            SetEnabled = (id, enabled) => Submit(worker.SetRetainerEnabled(id, enabled)),
        };
        settings = new SettingsPanel
        {
            SetChatEnabled = value => { config.NotificationChatEnabled = value; Plugin.Interface.SavePluginConfig(config); },
            SetChatIndex = index => { config.NotificationChatType = (int)GameNotifications.ChatTypes[index]; Plugin.Interface.SavePluginConfig(config); },
            SetPopupEnabled = value => { config.NotificationPopup = value; Plugin.Interface.SavePluginConfig(config); },
            SetSoundEnabled = value => { config.NotificationSound = value; Plugin.Interface.SavePluginConfig(config); },
            SetSoundId = value => { config.NotificationSoundId = value; Plugin.Interface.SavePluginConfig(config); },
            TestNotification = () =>
            {
                Plugin.TestNotification();
                if (!config.NotificationChatEnabled && !config.NotificationPopup && !config.NotificationSound)
                    uiStatus = "Game chat, popup, and sound outputs are all turned off. Enable at least one to preview alerts.";
            },
            SetItemContextMenu = value => { config.ItemContextMenuEnabled = value; Plugin.Interface.SavePluginConfig(config); },
            SetBellShortcut = value => { config.BellShortcutEnabled = value; Plugin.Interface.SavePluginConfig(config); },
            SetInventoryProgress = value =>
            {
                config.InventoryProgressEnabled = value;
                config.InventoryProgressGameBuild = value ? capture.GameBuild : null;
                worker.CancelQueuedInventory();
                Plugin.Interface.SavePluginConfig(config);
            },
            ConfirmInventoryBuild = () =>
            {
                config.InventoryProgressGameBuild = capture.GameBuild;
                worker.CancelQueuedInventory();
                Plugin.Interface.SavePluginConfig(config);
                uiStatus = "Inventory progress enabled for the current game build.";
            },
            SaveConnection = (nextLocalhost, nextOrigin, nextProfile) =>
            {
                try
                {
                    _ = new UltrosEndpoint(nextLocalhost, nextOrigin);
                    if (nextProfile.Length < 1 || nextProfile.Any(c => !char.IsAsciiLetterOrDigit(c) && c is not '-' and not '_'))
                        throw new ArgumentException();
                    config.Localhost = nextLocalhost;
                    config.LocalOrigin = nextOrigin;
                    config.Profile = nextProfile;
                    Plugin.Interface.SavePluginConfig(config);
                    uiStatus = "Connection settings saved. Reload Ultros in Dalamud to apply.";
                }
                catch { uiStatus = "Use a loopback origin and a profile containing letters, numbers, '-' or '_'."; }
            },
            OpenLab = Plugin.OpenLab,
        };

        Size = new Vector2(960, 720);
        SizeCondition = ImGuiCond.FirstUseEver;
        SizeConstraints = new WindowSizeConstraints { MinimumSize = new Vector2(650, 440), MaximumSize = new Vector2(float.MaxValue) };
    }

    private void Submit(bool accepted) { uiStatus = accepted ? "" : "The worker queue is full; retry shortly."; }

    private void Open(string url) => _ = OpenBrowser(url);

    private async Task OpenBrowser(string url)
    {
        if (!await BrowserLinks.Open(url).ConfigureAwait(false))
            Interlocked.Exchange(ref browserError, "Could not open your browser. Open the website manually or retry this button.");
    }

    private void OpenItemWebsite(int id, string scope)
    {
        var baseUri = new Uri(worker.View.VerificationUrl);
        var path = scope.Length > 0 ? $"/item/{Uri.EscapeDataString(scope)}/{id}" : $"/item/{id}";
        Open(new Uri(baseUri, path).AbsoluteUri);
    }

    private static bool HasItem(int id) => id > 0 && Plugin.Data.GetExcelSheet<Item>().HasRow((uint)id);
    private static string ItemName(int id) => Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id, out var item) ? item.Name.ToString() : $"Item #{id}";
    private static bool Marketable(int id) => id > 0 && Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id, out var item) && item.ItemSearchCategory.RowId != 0;
    private static string WorldName(int id) => id > 0 && Plugin.Data.GetExcelSheet<Lumina.Excel.Sheets.World>().TryGetRow((uint)id, out var world) ? world.Name.ToString() : $"World #{id}";

    private static (int Id, string Name)[] SearchMarketableItems(string term) =>
        Plugin.Data.GetExcelSheet<Item>()
            .Where(i => i.ItemSearchCategory.RowId != 0)
            .Select(i => (Id: checked((int)i.RowId), Name: i.Name.ToString()))
            .Where(i => i.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
            .Take(25)
            .ToArray();

    private static (int Id, string Name)[] SearchAllItems(string term) =>
        Plugin.Data.GetExcelSheet<Item>()
            .Where(i => i.RowId > 0)
            .Select(i => (Id: checked((int)i.RowId), Name: i.Name.ToString()))
            .Where(i => i.Name.Length > 0 && i.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
            .Take(25)
            .ToArray();

    private static ImTextureID? ItemIcon(int id, bool hq)
    {
        if (id <= 0 || !Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id, out var item) || item.Icon == 0)
            return null;
        var lookup = new GameIconLookup(item.Icon, hq);
        return Plugin.TextureProvider.GetFromGameIcon(lookup).TryGetWrap(out var wrap, out _) ? wrap.Handle : null;
    }

    internal void ShowItem(int id, string? priceWorld = null)
    {
        market.SelectItem(id, priceWorld);
        openItem = true;
        IsOpen = true;
    }

    internal void ShowRetainers()
    {
        openRetainers = true;
        manageRetainers = true;
        IsOpen = true;
    }

    internal void AddToList(int id, ListEditTarget? destination, string quality = "any", int quantity = 1)
    {
        openItem = false;
        openLists = true;
        IsOpen = true;
        uiStatus = destination is not null && worker.AddItem(destination, id, quality, quantity)
            ? ""
            : "Choose a writable list, then add the selected item. Enable list editing in Account if needed.";
    }

    public override void OnClose() => account.Reset();
    public override void OnOpen() => account.Reset();
    public override void PreDraw()
    {
        ImGui.SetNextWindowBgAlpha(1);
        UiKit.BeginStyle();
        styled = true;
    }

    public override void PostDraw()
    {
        if (!styled) return;
        styled = false;
        UiKit.EndStyle();
    }

    public override void Draw()
    {
        if (Interlocked.Exchange(ref browserError, null) is { } browserFailure) uiStatus = browserFailure;
        var v = worker.View;
        var activatePage = firstDraw || openItem || openLists || openRetainers;
        market.SyncScope(v);
        if (firstDraw) { page = v.Connected ? Page.Market : Page.Account; firstDraw = false; }
        if (openItem) { page = Page.Market; openItem = false; }
        if (openLists) { page = Page.Lists; openLists = false; }
        if (openRetainers) { page = Page.Retainers; openRetainers = false; }

        var unread = v.Inbox.Count(d => v.ReadNotificationIds?.Contains(d.Id) != true);
        page = UiKit.Navigation(page, v.Connected, unread, activatePage);
        account.Track(v, page == Page.Account);
        UiKit.BeginContent();
        try
        {
            if (config.Localhost) UiKit.Pill("LOCALHOST", UiKit.Warning);
            switch (page)
            {
                case Page.Market:
                    market.Draw(v);
                    break;
                case Page.Lists:
                    lists.Draw(v, InventoryBuildBanner());
                    break;
                case Page.Retainers:
                    var expand = manageRetainers;
                    manageRetainers = false;
                    retainerListings.Draw(v, BuildRetainerChrome(v, expand));
                    break;
                case Page.Notifications:
                    notifications.Draw(v);
                    break;
                case Page.Account:
                    account.Draw(v);
                    break;
                case Page.Settings:
                    settings.Draw(BuildSettingsState());
                    break;
            }
        }
        finally
        {
            UiKit.EndContent(v.Status, uiStatus.Length > 0 ? uiStatus : null, () => uiStatus = "");
        }
    }

    private (string Title, string Message, string ActionLabel, Action OnAction)? InventoryBuildBanner()
    {
        if (!config.InventoryProgressEnabled || !capture.Compatible || capture.GameBuild is null || config.InventoryProgressGameBuild == capture.GameBuild)
            return null;
        return (
            "Inventory tracking paused after a game update",
            $"FFXIV updated to build {capture.GameBuild}. Confirm to resume automatic acquired-item tracking for your active list.",
            "Resume inventory tracking for this build",
            () =>
            {
                config.InventoryProgressGameBuild = capture.GameBuild;
                worker.CancelQueuedInventory();
                Plugin.Interface.SavePluginConfig(config);
                uiStatus = "Inventory progress enabled for the current game build.";
            });
    }

    private RetainerChrome BuildRetainerChrome(ClientView v, bool expandManage)
    {
        var uploaderOn = config.CaptureEnabled && capture.Compatible && capture.GameBuild is not null && config.CaptureGameBuild == capture.GameBuild;
        (string Title, string Message, string? ActionLabel, Action? OnAction)? banner = null;
        if (!capture.Compatible)
        {
            banner = ("Plugin update needed for Dalamud client structs",
                capture.CompatibilityStatus + " Update Ultros in /xlplugins, then reload.", null, null);
        }
        else if (config.CaptureEnabled && capture.GameBuild is not null && config.CaptureGameBuild != capture.GameBuild)
        {
            banner = ("Listing uploader paused after a game update",
                $"FFXIV updated to build {capture.GameBuild}. Confirm once to resume private retainer uploads.",
                "Resume listing uploader for this build",
                () =>
                {
                    config.CaptureEnabled = true;
                    config.CaptureGameBuild = capture.GameBuild;
                    Plugin.Interface.SavePluginConfig(config);
                    uiStatus = "Listing uploader enabled for the current game build. Open a retainer's Markets window to sync.";
                });
        }

        return new RetainerChrome(
            UploaderOn: uploaderOn,
            CaptureCompatible: capture.Compatible,
            GameBuild: capture.GameBuild,
            CaptureStatus: capture.Status,
            ObservedCharacter: capture.Character,
            BellCharacter: capture.Bell?.Character,
            Banner: banner,
            ExpandManage: expandManage,
            Refresh: () => Submit(worker.RefreshRetainers()),
            OpenWebsite: () => Open(new Uri(new Uri(v.VerificationUrl), "/retainers/listings").AbsoluteUri),
            UseGameOrder: c => Submit(worker.UseGameRetainerOrder(c)),
            ToggleUploader: () =>
            {
                config.CaptureEnabled = !uploaderOn;
                config.CaptureGameBuild = capture.GameBuild;
                Plugin.Interface.SavePluginConfig(config);
                uiStatus = uploaderOn
                    ? "Listing uploader paused on this game client."
                    : "Listing uploader enabled. Close and reopen the retainer's Markets window to capture its listings.";
            },
            RegisterAll: c => Submit(worker.Register(c)),
            UploadConsent: (id, enabled, delete) => Submit(worker.UploadConsent(id, enabled, delete)));
    }

    private SettingsState BuildSettingsState()
    {
        var chatIndex = Math.Max(0, Array.FindIndex(GameNotifications.ChatTypes, t => (int)t == config.NotificationChatType));
        var canEnable = capture.Compatible && capture.GameBuild is not null;
        return new SettingsState(
            ChatEnabled: config.NotificationChatEnabled,
            ChatIndex: chatIndex,
            ChatLabels: GameNotifications.ChatLabels,
            PopupEnabled: config.NotificationPopup,
            SoundEnabled: config.NotificationSound,
            SoundId: config.NotificationSoundId,
            ItemContextMenuEnabled: config.ItemContextMenuEnabled,
            BellShortcutEnabled: config.BellShortcutEnabled,
            InventoryProgressEnabled: config.InventoryProgressEnabled,
            CanEnableInventory: canEnable,
            InventoryNeedsBuildConfirm: config.InventoryProgressEnabled && canEnable && config.InventoryProgressGameBuild != capture.GameBuild,
            InventoryStatus: inventoryProgress.Status,
            LocalhostAvailable: UltrosEndpoint.LocalhostAvailable,
            Localhost: config.Localhost,
            LocalOrigin: config.LocalOrigin,
            Profile: config.Profile,
            CaptureCompatibilityStatus: capture.CompatibilityStatus,
            GameBuild: capture.GameBuild);
    }
}
