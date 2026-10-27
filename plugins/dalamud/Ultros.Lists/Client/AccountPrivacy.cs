using System.Globalization;

namespace Ultros.Client;

/// <summary>Reveal is deliberately temporary and is never written to plugin configuration.</summary>
public sealed class AccountPrivacy
{
    private (string? Account,string? Device)? identity;
    public bool Revealed { get; set; }
    public void Track(DeviceSession? session,bool viewingAccount,bool connected)
    {
        var next=(session?.AccountId,session?.Id);
        if(!viewingAccount||!connected||identity!=next)Revealed=false;
        identity=next;
    }
    public void Reset(){Revealed=false;identity=null;}
    public static string MaskName(string name)
    {
        var chars=StringInfo.ParseCombiningCharacters(name);
        var visible=Math.Min(3,Math.Max(0,chars.Length-1));
        return (visible==0?"":name[..chars[visible]])+"*****";
    }
}
