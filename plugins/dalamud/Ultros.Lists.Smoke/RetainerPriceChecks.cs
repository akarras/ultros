using System.Text.Json;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class RetainerPriceChecks
{
    internal static void Run()
    {
        var now=DateTimeOffset.UtcNow;
        var wire=JsonSerializer.SerializeToUtf8Bytes(new {
            retainer_id="23",name="Fixture",world_id=63,world_name="Gilgamesh",source="in_game",
            observed_at=now,fetched_at=now,listings=new[]{new {
                slot=2,item_id=8,hq=false,quantity=5,current_price=100L,competitor_price=90L,
                market_checked_at=now,suggested_price=89L,status="fresh"
            }}
        });
        var lookup=JsonSerializer.Deserialize<RetainerPriceLookup>(wire,Protocol.Json)??throw new Exception("Price lookup did not deserialize.");
        var row=lookup.Listings.Single();
        if(lookup.RetainerId!="23"||lookup.WorldName!="Gilgamesh"||row.CurrentPrice!=100||!row.CanSuggest(now))
            throw new Exception("Price lookup lost listing metadata or rejected a current comparison.");
        if(row.CanSuggest(now.AddMinutes(31))||row.CanSuggest(now.AddMinutes(-2)))
            throw new Exception("An open window retained an expired/future comparison.");
        foreach(var invalid in new[]{row with{Status="stale"},row with{Status="unknown_age"},
            row with{SuggestedPrice=null},row with{SuggestedPrice=0},row with{MarketCheckedAt=null},row with{CompetitorPrice=null}})
            if(invalid.CanSuggest(now))throw new Exception("An incomplete or stale comparison offered a suggestion.");
    }
}
