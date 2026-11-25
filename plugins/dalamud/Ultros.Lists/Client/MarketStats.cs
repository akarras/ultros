using System.Globalization;
namespace Ultros.Client;

public sealed record MarketSummary(
    long? CheapestNq,
    long? CheapestHq,
    int NqCount,
    int HqCount,
    long? MedianSalePrice,
    long? AverageSalePrice,
    int SaleCount);

public static class MarketStats
{
    public static MarketSummary Summarize(MarketRow[] listings, SaleRow[] sales)
    {
        long? cheapestNq = null, cheapestHq = null;
        int nqCount = 0, hqCount = 0;
        foreach (var row in listings)
        {
            if (row.UnitPrice <= 0) continue;
            if (row.Hq)
            {
                hqCount++;
                if (cheapestHq is null || row.UnitPrice < cheapestHq) cheapestHq = row.UnitPrice;
            }
            else
            {
                nqCount++;
                if (cheapestNq is null || row.UnitPrice < cheapestNq) cheapestNq = row.UnitPrice;
            }
        }
        var validSales = sales.Where(s => s.UnitPrice > 0).Select(s => s.UnitPrice).OrderBy(p => p).ToArray();
        long? median = null, average = null;
        if (validSales.Length > 0)
        {
            var mid = validSales.Length / 2;
            median = validSales.Length % 2 == 1
                ? validSales[mid]
                : (validSales[mid - 1] + validSales[mid]) / 2;
            long sum = 0;
            foreach (var p in validSales) sum += p;
            average = sum / validSales.Length;
        }
        return new(cheapestNq, cheapestHq, nqCount, hqCount, median, average, validSales.Length);
    }
}

public static class TimestampFormat
{
    public static bool TryParseUtc(string raw, out DateTimeOffset timestamp)
    {
        if (string.IsNullOrWhiteSpace(raw)) { timestamp = default; return false; }
        return DateTimeOffset.TryParse(raw, CultureInfo.InvariantCulture,
            DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out timestamp);
    }

    public static string FormatSaleTime(string raw, DateTimeOffset? now = null) =>
        TryParseUtc(raw, out var parsed) ? FormatRelativeOrLocal(parsed, now ?? DateTimeOffset.UtcNow) : raw;

    public static string FormatRelativeOrLocal(DateTimeOffset at, DateTimeOffset? now = null)
    {
        var reference = now ?? DateTimeOffset.UtcNow;
        var age = reference - at;
        if (age < TimeSpan.FromSeconds(-60)) return at.ToLocalTime().ToString("MMM d, t", CultureInfo.CurrentCulture);
        if (age < TimeSpan.FromMinutes(1)) return "Just now";
        if (age < TimeSpan.FromHours(1)) return $"{Math.Max(1, (int)age.TotalMinutes)}m ago";
        if (age < TimeSpan.FromHours(24)) return $"{Math.Max(1, (int)age.TotalHours)}h ago";
        var local = at.ToLocalTime();
        var localNow = reference.ToLocalTime();
        if (local.Date == localNow.Date.AddDays(-1)) return $"Yesterday {local:t}";
        return local.Year == localNow.Year
            ? local.ToString("MMM d, t", CultureInfo.CurrentCulture)
            : local.ToString("MMM d, yyyy", CultureInfo.CurrentCulture);
    }
}
