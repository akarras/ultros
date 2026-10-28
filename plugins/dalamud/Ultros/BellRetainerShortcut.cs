using System.Numerics;
using Dalamud.Bindings.ImGui;
using Ultros.Client;
using UltrosPlugin.Ui;

namespace UltrosPlugin;

/// <summary>Managed overlay anchored to the bell header; no game UI nodes are modified.</summary>
internal sealed class BellRetainerShortcut : IDisposable
{
    private readonly ClientWorker worker;
    private readonly RetainerCapture capture;
    private readonly BellRetainerPanel panel;
    internal BellRetainerShortcut(ClientWorker worker,RetainerCapture capture,MainWindow window)
    {
        this.worker=worker;this.capture=capture;
        panel=new(){Refresh=character=>worker.Register(character),UseGameOrder=character=>worker.UseGameRetainerOrder(character),Manage=window.ShowRetainers};
        Plugin.Interface.UiBuilder.Draw+=Draw;
    }
    private void Draw()
    {
        var view=worker.View;
        if(!view.Connected||capture.Bell is not { } bell)return;
        ImGui.SetNextWindowPos(ImGui.GetMainViewport().Pos+bell.Position);
        ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding,Vector2.Zero);
        try {
            var visible=ImGui.Begin("###UltrosBellShortcut",ImGuiWindowFlags.NoDecoration|ImGuiWindowFlags.NoBackground|ImGuiWindowFlags.AlwaysAutoResize|ImGuiWindowFlags.NoSavedSettings|ImGuiWindowFlags.NoMove|ImGuiWindowFlags.NoNav|ImGuiWindowFlags.NoFocusOnAppearing);
            try {
                if(visible){ImGui.SetWindowFontScale(bell.Scale);panel.Draw(view,bell.Character);}
            }finally{ImGui.End();}
        }finally{ImGui.PopStyleVar();}
    }
    public void Dispose()=>Plugin.Interface.UiBuilder.Draw-=Draw;
}
