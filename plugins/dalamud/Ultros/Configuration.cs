using Dalamud.Configuration;
namespace UltrosPlugin;
[Serializable]
internal sealed class Configuration : IPluginConfiguration
{
    public int Version { get; set; } = 1;
    public string Profile { get; set; } = "main";
    public bool Localhost { get; set; }
    public string LocalOrigin { get; set; } = "http://localhost:8080";
    public bool CaptureEnabled { get; set; } = true;
    public string? CaptureGameBuild { get; set; }
    public bool InventoryProgressEnabled { get; set; }
    public bool ItemContextMenuEnabled { get; set; } = true;
    public string? InventoryProgressGameBuild { get; set; }
    public int NotificationChatType { get; set; } = (int)Dalamud.Game.Text.XivChatType.Echo;
    public bool NotificationPopup { get; set; }
    public bool NotificationSound { get; set; }
    public int NotificationSoundId { get; set; } = 2;
}
