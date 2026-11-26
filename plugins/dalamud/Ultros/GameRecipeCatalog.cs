using Lumina.Excel.Sheets;
using Ultros.Client;

namespace UltrosPlugin;

internal sealed class GameRecipeCatalog : ICraftRecipeSource
{
    private const uint CrystalSearchCategory = 58;
    private static readonly string[] CraftJobAbbrs = ["CRP", "BSM", "ARM", "GSM", "LTW", "WVR", "ALC", "CUL"];

    private readonly object gate = new();
    private Dictionary<int, RecipeInfo>? byRecipeId;
    private Dictionary<int, int>? defaultByItemId;
    private (int RecipeId, int ItemId, string Name, string JobAbbr, int Level)[]? searchable;

    public RecipeInfo? GetRecipe(int recipeId)
    {
        if (recipeId <= 0) return null;
        EnsureLoaded();
        return byRecipeId!.GetValueOrDefault(recipeId);
    }

    public RecipeInfo? GetDefaultRecipeForItem(int itemId)
    {
        if (itemId <= 0) return null;
        EnsureLoaded();
        return defaultByItemId!.TryGetValue(itemId, out var recipeId)
            ? byRecipeId!.GetValueOrDefault(recipeId)
            : null;
    }

    public bool IsCrystal(int itemId) =>
        itemId > 0
        && Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)itemId, out var item)
        && item.ItemSearchCategory.RowId == CrystalSearchCategory;

    internal (int RecipeId, int ItemId, string Name, string JobAbbr, int Level)[] SearchRecipes(string term)
    {
        if (string.IsNullOrWhiteSpace(term)) return [];
        EnsureLoaded();
        return searchable!
            .Where(r => r.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
            .Take(25)
            .ToArray();
    }

    private void EnsureLoaded()
    {
        if (byRecipeId is not null) return;
        lock (gate)
        {
            if (byRecipeId is not null) return;
            var recipesSheet = Plugin.Data.GetExcelSheet<Recipe>();
            var itemSheet = Plugin.Data.GetExcelSheet<Item>();
            var levelSheet = Plugin.Data.GetExcelSheet<RecipeLevelTable>();

            var recipesMap = new Dictionary<int, RecipeInfo>();
            var defaultsMap = new Dictionary<int, int>();
            var searchList = new List<(int RecipeId, int ItemId, string Name, string JobAbbr, int Level)>();

            foreach (var row in recipesSheet)
            {
                if (row.RowId == 0 || row.ItemResult.RowId == 0 || row.RowId > int.MaxValue || row.ItemResult.RowId > int.MaxValue)
                    continue;

                var recipeId = (int)row.RowId;
                var outputId = (int)row.ItemResult.RowId;
                var yieldAmt = Math.Max(1L, row.AmountResult);
                var jobAbbr = row.CraftType.RowId < CraftJobAbbrs.Length ? CraftJobAbbrs[row.CraftType.RowId] : "";
                var level = levelSheet.TryGetRow(row.RecipeLevelTable.RowId, out var lvlRow)
                    ? (int)lvlRow.ClassJobLevel
                    : 0;

                var count = Math.Min(row.Ingredient.Count, row.AmountIngredient.Count);
                var ingredients = new Dictionary<int, long>();
                for (var i = 0; i < count; i++)
                {
                    var ingId = row.Ingredient[i].RowId;
                    var ingAmt = row.AmountIngredient[i];
                    if (ingId == 0 || ingId > int.MaxValue || ingAmt == 0) continue;
                    var key = (int)ingId;
                    ingredients[key] = ingredients.GetValueOrDefault(key) + ingAmt;
                }

                var ingList = ingredients
                    .Select(kv => new RecipeIngredient(kv.Key, kv.Value))
                    .ToArray();

                recipesMap[recipeId] = new RecipeInfo(recipeId, outputId, yieldAmt, jobAbbr, level, ingList);

                if (!defaultsMap.TryGetValue(outputId, out var existingId) || recipeId < existingId)
                    defaultsMap[outputId] = recipeId;

                if (itemSheet.TryGetRow((uint)outputId, out var item))
                {
                    var name = item.Name.ToString();
                    if (name.Length > 0)
                        searchList.Add((recipeId, outputId, name, jobAbbr, level));
                }
            }

            searchable = searchList
                .OrderBy(r => r.Name, StringComparer.OrdinalIgnoreCase)
                .ThenBy(r => r.RecipeId)
                .ToArray();
            defaultByItemId = defaultsMap;
            byRecipeId = recipesMap;
        }
    }
}
