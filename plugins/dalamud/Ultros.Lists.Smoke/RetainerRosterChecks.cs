using System.Text.Json;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class RetainerRosterChecks
{
    private static void Require(bool value,string message){if(!value)throw new Exception(message);}
    internal static void Run()
    {
        ulong[] slots=[11,22,0,33,0,0,0,0,0,0];
        Require(RetainerRoster.DisplaySlots(slots,[3,0,1,0,0,0,0,0,0,0])!.SequenceEqual(new[]{3,0,1}),"Physical storage order replaced the game's display order.");
        Require(RetainerRoster.DisplaySlots(slots,[3,3,1,0,0,0,0,0,0,0]) is null,"Duplicate display slots were accepted.");
        Require(RetainerRoster.DisplaySlots(slots,[3,2,1,0,0,0,0,0,0,0]) is null,"An empty display slot was accepted.");
        Require(RetainerRoster.DisplaySlots(slots,[10,0,1,0,0,0,0,0,0,0]) is null,"An out-of-range display slot was read.");
        var now=DateTimeOffset.UtcNow;
        var current=new ObservedCharacter("11","Fixture",63,[new("22","Second",1,0,true),new("33","First",1,1,false)]);
        var saved=new UnverifiedCharacter("1","Fixture",63,true,false,true,2,now,[new("1","First",1,true,1,now,false,1,1,false,true),new("2","Second",1,true,1,now,false,2,0,true)]);
        Require(RetainerRoster.GameOrder(saved,current)!.SequenceEqual(new[]{"2","1"}),"Reordering dropped a disabled retainer or ignored the game order.");
        Require(RetainerRoster.Missing([saved],current)==0&&!RetainerRoster.Changed([saved],current),"An explicitly disabled/unavailable retainer caused automatic re-registration.");
        Require(!RetainerRoster.Changed([saved],current with{Retainers=current.Retainers.Select(r=>r with{Available=null,GameOrder=null}).ToArray()}),"Unknown roster fields caused a repeated refresh loop.");
        var missing=current with{Retainers=[..current.Retainers,new("44","New retainer",1,2,true)]};
        Require(RetainerRoster.Missing([saved],missing)==1&&RetainerRoster.Changed([saved],missing),"A newly hired retainer did not enable one-click refresh.");
        Require(RetainerRoster.GameOrder(saved,missing) is null,"Unknown retainers were silently omitted from the requested order.");
        var json=JsonSerializer.Serialize(new ObservedRetainer("1","Old client",1),Protocol.Json);
        Require(!json.Contains("game_order")&&!json.Contains("available"),"Legacy registrations gained unsupported optional fields.");
        Console.WriteLine("PASS: validated bell order, missing retainers, disabled-retainer preservation and legacy observation shape.");
    }
}
