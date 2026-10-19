using System.Numerics;
using Dalamud.Bindings.ImGui;
using Dalamud.Interface.Utility;
using Dalamud.Interface.Windowing;
using Item = Lumina.Excel.Sheets.Item;
using Ultros.Lists;

namespace UltrosPlugin;

internal sealed class MergeLabWindow : Window, IDisposable
{
    private readonly LocalCartStore store;
    private string profile = "main";
    private string name = "Merge experiment";
    private string status;
    private int itemId = 8;
    private int quantity = 10;
    private int quality;
    private string search = "";
    private (uint Id, string Name)[] matches = [];
    private readonly Dictionary<string,(int Need,int Acquired)> drafts=[];
    private static readonly string[] Qualities = ["any", "nq", "hq"];

    public MergeLabWindow(LocalCartStore store, string? error) : base("Ultros · local merge lab###UltrosMergeLab")
    {
        this.store = store;
        status = error ?? "Ready. Cart A saves locally after every edit; peer B is a scratch branch.";
        Size = new Vector2(860, 620);
        SizeCondition = ImGuiCond.FirstUseEver;
        SizeConstraints = new WindowSizeConstraints { MinimumSize = new Vector2(650, 420), MaximumSize = new Vector2(float.MaxValue) };
    }

    private void Run(Action action, string success)
    {
        try { action(); status = success; }
        catch (Exception error) { status = error.Message; }
    }

    private void DrawStatus()
    {
        ImGui.PushTextWrapPos();
        try { ImGui.TextUnformatted(status); }
        finally { ImGui.PopTextWrapPos(); }
    }

    public override void Draw()
    {
        ImGui.TextWrapped("Local-only Rust CRDT experiment. No Ultros login, live market connection, uploads, or game automation.");
        ImGui.SetNextItemWidth(140 * ImGuiHelpers.GlobalScale);
        ImGui.InputText("Profile", ref profile, 32);
        ImGui.SameLine();
        if (ImGui.Button("Open profile")) Run(() => store.OpenProfile(profile), "Profile opened. Each game process needs a different profile.");
        ImGui.SameLine();
        if (ImGui.Button("Test notification")) Run(Plugin.TestNotification, "Test notification sent.");
        ImGui.Separator();
        if (store.Main is null)
        {
            DrawStatus();
            ImGui.TextWrapped("If main is in use or its saved data is invalid, choose another profile. Invalid files are preserved.");
            return;
        }
        ImGui.TextUnformatted($"Profile: {store.Profile} · {(store.Dirty ? "UNSAVED — retry Save" : "saved locally")}");
        if (ImGui.Button("Save")) Run(store.Save, "Saved.");
        ImGui.SameLine();
        if (ImGui.Button("Reopen saved cart")) Run(store.Reload, "Reopened with a fresh CRDT writer identity. Scratch peer cleared.");
        ImGui.SameLine();
        if (ImGui.Button("Fork A into peer B")) Run(store.Fork, "Peer B forked. Edit A and B independently, then merge.");
        if (ImGui.Button("Copy A snapshot")) Run(() => ImGui.SetClipboardText(Convert.ToBase64String(store.Main!.Export())), "Raw Loro snapshot copied as base64.");
        ImGui.SameLine();
        if (ImGui.Button("Load clipboard as peer B")) Run(() => store.LoadComparison(ImGui.GetClipboardText()), "Clipboard loaded as a separate peer. A is unchanged.");
        ImGui.Separator();
        if (ImGui.BeginTabBar("peers"))
        {
            try
            {
                if (ImGui.BeginTabItem("Cart A · saved"))
                {
                    try { DrawCart(store.Main!, "A"); } finally { ImGui.EndTabItem(); }
                }
                if (ImGui.BeginTabItem("Peer B · scratch"))
                {
                    try
                    {
                        if (store.Comparison is { } peer) DrawCart(peer, "B");
                        else ImGui.TextWrapped("Fork A into peer B to try offline/concurrent edits. Peer B is not saved.");
                    }
                    finally { ImGui.EndTabItem(); }
                }
                if (ImGui.BeginTabItem("Merge lab"))
                {
                    try { DrawMerge(); } finally { ImGui.EndTabItem(); }
                }
            }
            finally { ImGui.EndTabBar(); }
        }
        ImGui.Separator();
        DrawStatus();
    }

    private void DrawCart(Document document, string label)
    {
        var view = document.View;
        ImGui.TextUnformatted($"{view.Name} · peer {view.PeerId}");
        ImGui.SetNextItemWidth(220 * ImGuiHelpers.GlobalScale);
        ImGui.InputText("List name", ref name, 128);
        ImGui.SameLine();
        if (ImGui.Button("Rename")) Apply(document, new { kind = "rename", name });
        ImGui.SetNextItemWidth(220 * ImGuiHelpers.GlobalScale);
        var find = ImGui.InputTextWithHint("##item-search", "Item name (at least 2 characters)", ref search, 100, ImGuiInputTextFlags.EnterReturnsTrue);
        ImGui.SameLine();
        find |= ImGui.Button("Find item");
        if (find) Run(() =>
        {
            var term = search.Trim();
            if (term.Length < 2) throw new ArgumentException("Enter at least two characters to find an item.");
            matches = Plugin.Data.GetExcelSheet<Item>()
                .Select(item => (Id: item.RowId, Name: item.Name.ToString()))
                .Where(item => item.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
                .Take(25).ToArray();
        }, "Search complete (up to 25 matches). Select a match, then Add.");
        if (matches.Length > 0 && ImGui.BeginCombo("Matches", "Choose an item"))
        {
            try
            {
                foreach (var match in matches)
                    if (ImGui.Selectable($"{match.Name} (#{match.Id})", itemId == match.Id)) itemId = checked((int)match.Id);
            }
            finally { ImGui.EndCombo(); }
        }
        ImGui.SetNextItemWidth(90 * ImGuiHelpers.GlobalScale);
        ImGui.InputInt("Item ID", ref itemId);
        ImGui.SameLine();
        ImGui.SetNextItemWidth(85 * ImGuiHelpers.GlobalScale);
        ImGui.Combo("Quality", ref quality, Qualities, Qualities.Length);
        ImGui.SameLine();
        ImGui.SetNextItemWidth(80 * ImGuiHelpers.GlobalScale);
        ImGui.InputInt("Quantity", ref quantity);
        ImGui.SameLine();
        if (ImGui.Button("Add")) Apply(document, new { kind = "add", item_id = itemId, quality = Qualities[quality], need = quantity });
        if (ImGui.Button("Undo")) Apply(document, new { kind = "undo" });
        ImGui.SameLine();
        if (ImGui.Button("Redo")) Apply(document, new { kind = "redo" });
        ImGui.SameLine();
        ImGui.TextUnformatted("Acquired uses counters; simultaneous purchases add together.");
        if (!ImGui.BeginTable("rows-v2-" + label, 6, ImGuiTableFlags.Borders | ImGuiTableFlags.RowBg | ImGuiTableFlags.SizingStretchProp)) return;
        try
        {
            var scale=ImGuiHelpers.GlobalScale;
            ImGui.TableSetupColumn("Item",ImGuiTableColumnFlags.WidthStretch,1);
            ImGui.TableSetupColumn("Quality",ImGuiTableColumnFlags.WidthFixed,60*scale);
            ImGui.TableSetupColumn("Need",ImGuiTableColumnFlags.WidthFixed,80*scale);
            ImGui.TableSetupColumn("Acquired",ImGuiTableColumnFlags.WidthFixed,80*scale);
            ImGui.TableSetupColumn("Mark acquired",ImGuiTableColumnFlags.WidthFixed,100*scale);
            ImGui.TableSetupColumn("",ImGuiTableColumnFlags.WidthFixed,75*scale);
            ImGui.TableHeadersRow();
            foreach (var row in document.View.Rows)
            {
                ImGui.PushID($"{label}-{row.ItemId}-{row.Quality}");
                try
                {
                    ImGui.TableNextRow(); ImGui.TableNextColumn();
                    var text = Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)row.ItemId, out var item) ? item.Name.ToString() : $"Item #{row.ItemId}";
                    ImGui.TextUnformatted(text);
                    ImGui.TableNextColumn(); ImGui.TextUnformatted(row.Quality);
                    ImGui.TableNextColumn();
                    var draftKey=$"{label}/{row.ItemId}/{row.Quality}";
                    (int Need,int Acquired) draft=drafts.GetValueOrDefault(draftKey,((int)Math.Clamp(row.Need,int.MinValue,int.MaxValue),(int)Math.Clamp(row.Acquired,int.MinValue,int.MaxValue)));
                    ImGui.SetNextItemWidth(-1);
                    var commit=ImGui.InputInt("##need",ref draft.Need,0,0,"%d",ImGuiInputTextFlags.EnterReturnsTrue);
                    var needActive=ImGui.IsItemActive();commit|=ImGui.IsItemDeactivatedAfterEdit();
                    if(commit)Apply(document,new{kind="set_need",item_id=row.ItemId,quality=row.Quality,need=draft.Need});
                    ImGui.TableNextColumn();
                    ImGui.SetNextItemWidth(-1);
                    var acquired=ImGui.InputInt("##acquired",ref draft.Acquired,0,0,"%d",ImGuiInputTextFlags.EnterReturnsTrue);
                    var acquiredActive=ImGui.IsItemActive();acquired|=ImGui.IsItemDeactivatedAfterEdit();
                    if(acquired)Apply(document,new{kind="set_acquired",item_id=row.ItemId,quality=row.Quality,value=draft.Acquired});
                    if(!commit&&!acquired&&(needActive||acquiredActive))drafts[draftKey]=draft;else drafts.Remove(draftKey);
                    ImGui.TableNextColumn();
                    if (ImGui.Button("+1")) Apply(document, new { kind = "add_acquired", item_id = row.ItemId, quality = row.Quality, delta = 1 });
                    ImGui.SameLine();
                    if (ImGui.Button("+5")) Apply(document, new { kind = "add_acquired", item_id = row.ItemId, quality = row.Quality, delta = 5 });
                    ImGui.TableNextColumn();
                    if (ImGui.Button("Remove")) Apply(document, new { kind = "remove", item_id = row.ItemId, quality = row.Quality });
                }
                finally { ImGui.PopID(); }
            }
        }
        finally { ImGui.EndTable(); }
    }

    private void Apply(Document document, object edit) => Run(() => store.Edit(document, edit),
        ReferenceEquals(document, store.Main) ? "Cart A saved locally." : "Peer B edited locally; it has not been merged into A.");

    private void DrawMerge()
    {
        ImGui.TextWrapped("Fork A, then add purchases on both peers. Merge repeatedly: the original operations should count once. Need and Target use the latest edit.");
        if (store.Comparison is null) return;
        if (ImGui.Button("Merge B into A")) Run(store.MergeIntoMain, "Merged B into A and saved A. Repeating imports is idempotent.");
        ImGui.SameLine();
        if (ImGui.Button("Merge A into B")) Run(store.MergeIntoComparison, "Merged A into B.");
        ImGui.SameLine();
        if (ImGui.Button("Exchange both ways")) Run(() => { store.MergeIntoMain(); store.MergeIntoComparison(); }, "Exchanged both ways.");
        var a = store.Main!.View; var b = store.Comparison.View;
        ImGui.TextUnformatted(a.Name == b.Name && a.Rows.SequenceEqual(b.Rows) ? "Views have converged." : "Views differ; merge to exchange operations.");
        ImGui.TextWrapped("To try two game clients: use different profile names, copy one client's snapshot, and load it as peer B in the other. No network listener is running.");
    }

    public void Dispose() => store.Dispose();
}
