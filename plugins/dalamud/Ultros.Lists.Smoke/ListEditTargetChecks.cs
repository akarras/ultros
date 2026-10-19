using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class ListEditTargetChecks
{
    internal static void Run()
    {
        static void Require(bool condition,string message){if(!condition)throw new Exception(message);}
        var now=DateTimeOffset.UtcNow;
        var session=new DeviceSession("device","Fixture","account","Fixture",["lists:read","lists:write"],now,now,now.AddDays(1),null);
        var target=ListEditTarget.Capture(session,1,true)!;
        Require(target.Matches(session,1,true),"Initial list destination rejected.");
        Require(!target.Matches(session with{AccountId="other"},1,true),"Queued edit accepted another account.");
        Require(!target.Matches(session with{Id="other"},1,true),"Queued edit accepted another device.");
        Require(!target.Matches(session,2,true),"Open menu silently changed its destination list.");
        Require(!target.Matches(session,1,false),"Paused/recovering list accepted a queued edit.");
        Require(!target.Matches(session with{Scopes=["lists:read"]},1,true),"Read-only scope accepted a queued edit.");
        Require(ListEditTarget.Capture(null,1,true) is null&&ListEditTarget.Capture(session,null,true) is null,"Missing destination accepted.");
    }
}
