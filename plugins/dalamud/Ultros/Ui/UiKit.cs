using System.Numerics;
using Dalamud.Bindings.ImGui;

namespace UltrosPlugin.Ui;

internal enum Page { Market, Lists, Retainers, Notifications, Account, Settings }

internal static class UiKit
{
    internal static float Scale=>ImGui.GetFontSize()/16f;
    internal static void BeginStyle()
    {
        ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding,new Vector2(16,14)*Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.FramePadding,new Vector2(10,6)*Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing,new Vector2(10,9)*Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.FrameRounding,4*Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ChildRounding,6*Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.CellPadding,new Vector2(10,7)*Scale);
    }
    internal static void EndStyle()=>ImGui.PopStyleVar(6);
    internal static void Muted(string text)=>ImGui.TextColored(ImGui.GetStyle().Colors[(int)ImGuiCol.TextDisabled],text);
    internal static void Hint(string text)
    {
        ImGui.PushStyleColor(ImGuiCol.Text,ImGui.GetStyle().Colors[(int)ImGuiCol.TextDisabled]);
        ImGui.TextWrapped(text);ImGui.PopStyleColor();
    }
    internal static void Heading(string title,string? hint=null)
    {
        ImGui.SetWindowFontScale(1.45f);ImGui.TextUnformatted(title);ImGui.SetWindowFontScale(1);
        if(hint is not null)Hint(hint);
        ImGui.Spacing();ImGui.Separator();ImGui.Spacing();
    }
    internal static void Section(string title)
    {
        ImGui.Spacing();ImGui.Separator();ImGui.Spacing();
        ImGui.SetWindowFontScale(1.3f);ImGui.TextUnformatted(title);ImGui.SetWindowFontScale(1);ImGui.Spacing();
    }
    internal static void Inline(float room=180)
    {
        var right=ImGui.GetWindowPos().X+ImGui.GetWindowContentRegionMax().X;
        if(ImGui.GetItemRectMax().X+room*Scale<right)ImGui.SameLine();
    }
    internal static void Empty(string title,string help)
    {
        ImGui.Spacing();ImGui.Spacing();ImGui.TextUnformatted(title);Hint(help);
    }
    internal static void BeginContent()
    {
        ImGui.BeginChild("page",Vector2.Zero,false);
        ImGui.BeginChild("content",new Vector2(0,-3*ImGui.GetTextLineHeightWithSpacing()),false);
    }
    internal static void EndContent(string status)
    {
        ImGui.EndChild();ImGui.Separator();Hint(status);ImGui.EndChild();
    }
    internal static Page Navigation(Page current,bool connected,int notifications,bool activate=false)
    {
        if(ImGui.BeginTabBar("ultros-pages",ImGuiTabBarFlags.FittingPolicyScroll))
        {
            try
            {
                var requested=current;
                foreach(var (page,label) in new[]{(Page.Account,"Account"),(Page.Market,"Market"),(Page.Lists,"Lists"),(Page.Retainers,"Retainers"),(Page.Notifications,"Notifications"),(Page.Settings,"Settings")})
                    if(ImGui.BeginTabItem(label,(activate||ImGui.IsWindowAppearing())&&requested==page?ImGuiTabItemFlags.SetSelected:ImGuiTabItemFlags.None))
                    {current=page;ImGui.EndTabItem();}
            }finally{ImGui.EndTabBar();}
        }
        return current;
    }
}
