using System.Diagnostics;
namespace Ultros.Client;

/// <summary>Never log a credential-bearing URL or a shell-launch exception.</summary>
public static class BrowserLinks
{
    public static Task<bool> Open(string url)=>Open(url,start=>{using var process=Process.Start(start);return true;});
    internal static Task<bool> Open(string url,Func<ProcessStartInfo,bool> launch)=>Task.Run(()=>
    {
        if(!Uri.TryCreate(url,UriKind.Absolute,out var uri)||uri.Scheme is not ("https" or "http")||uri.UserInfo.Length!=0)return false;
        try{return launch(new ProcessStartInfo(url){UseShellExecute=true,Verb="open"});}
        catch{return false;}
    });
}
