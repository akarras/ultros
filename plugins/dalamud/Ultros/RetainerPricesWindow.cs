using System.Globalization;
using System.Numerics;
using Dalamud.Bindings.ImGui;
using Dalamud.Interface.Windowing;
using Ultros.Client;
using Item=Lumina.Excel.Sheets.Item;

namespace UltrosPlugin;

internal sealed class RetainerPricesWindow(ClientWorker worker,MainWindow main) : Window("Retainer prices###UltrosRetainerPrices")
{
    private string? selected;
    private string feedback="";
    internal void Show(string retainer)
    {
        selected=retainer;feedback=worker.LookupRetainer(retainer)?"":"Could not queue the lookup. Reconnect with retainer reading enabled or retry shortly.";
        Size=new Vector2(1000,440);SizeCondition=ImGuiCond.FirstUseEver;
        SizeConstraints=new WindowSizeConstraints{MinimumSize=new Vector2(680,300),MaximumSize=new Vector2(float.MaxValue)};
        IsOpen=true;
    }
    public override void Draw()
    {
        var view=worker.View;
        if(!view.Connected||!view.Scopes.Contains("retainers:read")) {
            ImGui.TextWrapped("Connect with 'View characters and retainer listings' enabled in Account to use this window.");return;
        }
        if(selected is null)return;
        if(ImGui.Button("Refresh listings & prices"))feedback=worker.LookupRetainer(selected)?"":"The worker queue is full; retry shortly.";
        if(feedback.Length>0)ImGui.TextWrapped(feedback);
        if(view.RetainerPriceStatus.Length>0)ImGui.TextWrapped(view.RetainerPriceStatus);
        if(view.RetainerPrices is not { } lookup||lookup.RetainerId!=selected)return;
        ImGui.Separator();ImGui.TextUnformatted($"{lookup.Name} · {lookup.WorldName}");
        var source=lookup.Source=="in_game"?"Last captured in game":"Cached Universalis listings";
        ImGui.TextWrapped($"{source} · {(lookup.ObservedAt is { } observed?observed.ToLocalTime().ToString("g"):"no listing snapshot")} · lookup {lookup.FetchedAt.ToLocalTime():t}");
        ImGui.TextWrapped("Open this retainer's Markets window with uploads enabled to replace the last capture. This window does not detect sales or price changes by itself.");
        if(lookup.Listings.Length==0)ImGui.TextWrapped("No captured or public listings yet. Enable private uploads for this character and open its retainer's Markets window.");
        else if(ImGui.BeginTable("retainer-prices",6,ImGuiTableFlags.RowBg|ImGuiTableFlags.Borders|ImGuiTableFlags.ScrollX)) {
            try {
                ImGui.TableSetupColumn("Item",ImGuiTableColumnFlags.WidthStretch,1);
                ImGui.TableSetupColumn("Qty",ImGuiTableColumnFlags.WidthFixed,40);
                ImGui.TableSetupColumn("Listed / unit",ImGuiTableColumnFlags.WidthFixed,95);
                ImGui.TableSetupColumn("Other / unit",ImGuiTableColumnFlags.WidthFixed,95);
                ImGui.TableSetupColumn("Market reviewed",ImGuiTableColumnFlags.WidthFixed,140);
                ImGui.TableSetupColumn("Suggestion / unit",ImGuiTableColumnFlags.WidthFixed,200);ImGui.TableHeadersRow();
                foreach(var row in lookup.Listings) {
                    ImGui.PushID(row.Slot);
                    try {
                        ImGui.TableNextRow();ImGui.TableNextColumn();
                        var name=Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)row.ItemId,out var item)?item.Name.ToString():$"Item #{row.ItemId}";
                        if(ImGui.Selectable($"{name.Replace('#','＃')}{(row.Hq?" (HQ)":"")}##item"))main.ShowItem(row.ItemId,lookup.WorldName);
                        ImGui.TableNextColumn();ImGui.TextUnformatted(row.Quantity.ToString("N0"));
                        ImGui.TableNextColumn();ImGui.TextUnformatted(row.CurrentPrice.ToString("N0"));
                        ImGui.TableNextColumn();ImGui.TextUnformatted(row.CompetitorPrice?.ToString("N0")??"—");
                        ImGui.TableNextColumn();ImGui.TextUnformatted(row.MarketCheckedAt?.ToLocalTime().ToString("g")??"Unknown");
                        ImGui.TableNextColumn();
                        if(row.CanSuggest(DateTimeOffset.UtcNow)) {
                            ImGui.TextUnformatted(row.SuggestedPrice!.Value.ToString("N0"));ImGui.SameLine();
                            if(ImGui.SmallButton("Copy")) {
                                ImGui.SetClipboardText(row.SuggestedPrice.Value.ToString(CultureInfo.InvariantCulture));
                                feedback="Unit price copied. Enter it in game and confirm the listing there.";
                            }
                        }else ImGui.TextUnformatted(row.Status switch{"no_competitors"=>"No other listings","unknown_age"=>"Market age unknown",_=>"Stale — check marketboard"});
                    }finally{ImGui.PopID();}
                }
            }finally{ImGui.EndTable();}
        }
        ImGui.Separator();ImGui.TextWrapped("Suggestion: one gil below the cheapest other listing of the same quality, with a minimum of 1 gil. Requires market data reviewed within 30 minutes. Check the marketboard before choosing a price.");
        ImGui.TextWrapped("Copy a suggestion, enter it in game and confirm each listing there.");
    }
}
