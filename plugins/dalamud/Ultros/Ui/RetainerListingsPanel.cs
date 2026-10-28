using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

/// <summary>One stable listing view for all retainers; no per-retainer lookup window.</summary>
internal sealed class RetainerListingsPanel
{
    private bool showDisabled;
    internal Func<int,string> ItemName { get; init; }=id=>$"Item #{id}";
    internal Func<int,string> WorldName { get; init; }=id=>$"World #{id}";
    internal Func<int,bool> Marketable { get; init; }=_=>false;
    internal Action<int> OpenItem { get; init; }=_=>{};
    internal Action<string,bool> SetEnabled { get; init; }=(_,_)=>{};

    internal void Draw(ClientView view)
    {
        if(!view.Connected){UiKit.Empty("Connect to see your listings","Open Account to connect this game client to Ultros.");return;}
        if(!view.Scopes.Contains("retainers:read")){UiKit.Empty("Retainer access is not approved","Enable View retainer listings in Account and approve the updated connection.");return;}
        if(view.Characters.Length==0){UiKit.Empty("No retainers saved yet","Visit a summoning bell. Your characters and retainers sync automatically.");return;}
        if(view.Characters.SelectMany(c=>c.Retainers).Any(r=>r.Disabled))ImGui.Checkbox("Show disabled retainers",ref showDisabled);
        foreach(var character in view.Characters)
        {
            var visible=character.Retainers.Where(r=>showDisabled||!r.Disabled).ToArray();
            if(visible.Length==0)continue;
            ImGui.PushID(character.Id);
            try
            {
                UiKit.Section($"{character.Name} · {WorldName(character.HomeWorld)}");
                if(!character.UploadEnabled)ImGui.TextUnformatted("Uploads paused for this character.");
                foreach(var retainer in visible)
                {
                    ImGui.PushID(retainer.Id);
                    try
                    {
                        ImGui.Spacing();ImGui.SetWindowFontScale(1.15f);ImGui.TextUnformatted(retainer.Name);ImGui.SetWindowFontScale(1);
                        if(retainer.Disabled)UiKit.Hint("Disabled · saved listings kept; uploads paused.");
                        else if(retainer.Available==false)UiKit.Hint("Unavailable in game at the last bell visit. Disable it in Manage uploads if you no longer use it.");
                        if(retainer.GroupingConflict){UiKit.Hint("Saved under another character. Review Manage uploads below.");continue;}
                        if(!retainer.Registered){UiKit.Hint("Waiting for this retainer to sync with your account.");continue;}
                        var snapshot=view.RetainerSnapshots?.FirstOrDefault(s=>s.RetainerId==retainer.Id);
                        if(snapshot is null){UiKit.Hint(view.RetainerSnapshots is null?view.RefreshingRetainers?"Loading uploaded listings…":"Refresh listings to load this retainer's last upload.":"Open this retainer's Markets window to upload its listings.");continue;}
                        UiKit.Hint($"Last uploaded {snapshot.ObservedAt.ToLocalTime():g} · {snapshot.Listings.Length} / 20 listings");
                        if(snapshot.Listings.Length==0){ImGui.TextUnformatted("No items for sale at the last upload.");continue;}
                        Listings(snapshot.Listings);
                    }finally{ImGui.PopID();}
                }
            }finally{ImGui.PopID();}
        }
    }
    internal void Manage(UnverifiedCharacter character,bool canManage)
    {
        UiKit.Hint("Disable retainers you no longer use. Listings and upload data are kept so you can enable them again.");
        foreach(var retainer in character.Retainers)
        {
            ImGui.PushID(retainer.Id);
            try {
                ImGui.BeginDisabled(!canManage||!retainer.Registered||retainer.GroupingConflict);
                var enabled=!retainer.Disabled;
                if(ImGui.Checkbox($"{retainer.Name.Replace('#','＃')}##enabled",ref enabled))SetEnabled(retainer.Id,enabled);
                ImGui.EndDisabled();
                if(retainer.Available==false){ImGui.SameLine();UiKit.Muted("Unavailable in game");}
            }finally{ImGui.PopID();}
        }
    }
    private void Item(PrivateListing row)
    {
        var name=ItemName(row.ItemId).Replace('#','＃');
        if(Marketable(row.ItemId))
        {
            if(ImGui.Selectable($"{name}##item"))OpenItem(row.ItemId);
        }
        else ImGui.TextWrapped(name);
        if(ImGui.IsItemHovered())ImGui.SetTooltip(ItemName(row.ItemId));
    }
    private void Listings(PrivateListing[] rows)
    {
        if(ImGui.GetContentRegionAvail().X<620*UiKit.Scale)
        {
            foreach(var row in rows)
            {
                ImGui.PushID(row.Slot);
                try
                {
                    Item(row);
                    ImGui.TextUnformatted($"{(row.Hq?"HQ":"NQ")} · {row.Quantity:N0} × {row.UnitPrice:N0} gil");
                    UiKit.Hint($"Total {(row.UnitPrice*row.Quantity):N0} gil");
                    ImGui.Separator();
                }finally{ImGui.PopID();}
            }
            return;
        }
        if(!ImGui.BeginTable("listings",5,ImGuiTableFlags.RowBg|ImGuiTableFlags.BordersInnerH|ImGuiTableFlags.SizingStretchProp))return;
        try
        {
            ImGui.TableSetupColumn("Item",ImGuiTableColumnFlags.WidthStretch,1);
            ImGui.TableSetupColumn("Quality",ImGuiTableColumnFlags.WidthFixed,55*UiKit.Scale);
            ImGui.TableSetupColumn("Unit price",ImGuiTableColumnFlags.WidthFixed,95*UiKit.Scale);
            ImGui.TableSetupColumn("Qty",ImGuiTableColumnFlags.WidthFixed,40*UiKit.Scale);
            ImGui.TableSetupColumn("Total",ImGuiTableColumnFlags.WidthFixed,105*UiKit.Scale);
            ImGui.TableHeadersRow();
            foreach(var row in rows)
            {
                ImGui.PushID(row.Slot);
                try
                {
                    ImGui.TableNextRow();ImGui.TableNextColumn();Item(row);
                    ImGui.TableNextColumn();ImGui.TextUnformatted(row.Hq?"HQ":"NQ");
                    ImGui.TableNextColumn();ImGui.TextUnformatted(row.UnitPrice.ToString("N0"));
                    ImGui.TableNextColumn();ImGui.TextUnformatted(row.Quantity.ToString("N0"));
                    ImGui.TableNextColumn();ImGui.TextUnformatted((row.UnitPrice*row.Quantity).ToString("N0"));
                }finally{ImGui.PopID();}
            }
        }finally{ImGui.EndTable();}
    }
}
