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
        var cart=new CartView("Cart","peer",false,false,[]){Ready=true,AwaitingPersist=true};
        var view=new ClientView(true,"","",null,"https://ultros.app/plugin/verify",session,
            [new(new(1,1,"Cart",default),"Owner",null)],1,cart,false,[],[],null,[],null,[]);
        var recovery=ListEditTarget.CaptureDestination(view)!;
        Require(recovery.MatchesDestination(session,1)&&!recovery.MatchesDestination(session,2),"Queued recovery followed a different active list.");
        Require(!recovery.MatchesDestination(session with{Id="replacement"},1),"Queued recovery followed a replacement device.");
        var firstInstance=Guid.NewGuid();
        var instanceView=view with{CanEdit=true,CartInstance=firstInstance};
        var instanceTarget=ListEditTarget.Capture(instanceView)!;
        Require(instanceTarget.Matches(instanceView),"Instance-bound add-item target rejected its displayed cart.");
        Require(!instanceTarget.Matches(instanceView with{CartInstance=Guid.NewGuid()}),"A-B-A reopen accepted a stale add-item target.");
        Require(!instanceTarget.Matches(session,1,true,Guid.NewGuid()),"Worker destination check ignored the captured document instance.");
        Require(!target.Matches(session with{AccountId="other"},1,true),"Queued edit accepted another account.");
        Require(!target.Matches(session with{Id="other"},1,true),"Queued edit accepted another device.");
        Require(!target.Matches(session,2,true),"Open menu silently changed its destination list.");
        Require(!target.Matches(session,1,false),"Paused/recovering list accepted a queued edit.");
        Require(!target.Matches(session with{Scopes=["lists:read"]},1,true),"Read-only scope accepted a queued edit.");
        Require(ListEditTarget.Capture(null,1,true) is null&&ListEditTarget.Capture(session,null,true) is null,"Missing destination accepted.");
    }
}
