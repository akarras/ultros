# Leve Analyzer

The **Leve Analyzer** is a tool for identifying profitable crafting Levequests. It calculates the potential profit by comparing the cost of acquiring items (from the market board) with the gil reward and item rewards from turning them in.

## How to Use

1.  **Access the Tool**: Navigate to the [Leve Analyzer](/leve-analyzer) from the Apps menu.
2.  **Select World**: Choose the world you want to check prices on.
3.  **Job Filter**: Use the dropdown to filter by a specific crafting job (e.g., Culinarian, Alchemist).
4.  **Minimum Profit**: Set a threshold to hide low-profit levequests.

## How Profit is Calculated

The tool calculates profit using the following formula:

**Profit = Revenue - Cost**

-   **Revenue**: The Gil reward from the levequest + the estimated value of the reward items (based on market prices and probabilities).
-   **Cost**: The market board price of the required turn-in items.

## Understanding the Data

-   **Leve / Item**: The name of the levequest and the item required.
-   **Profit**: The estimated net profit.
-   **Revenue**: Total value you get back (Gil + Items).
-   **Cost**: Total cost to buy the items.
-   **Level**: The level of the levequest and the job category.

## Notes

-   This tool assumes you are buying the turn-in items from the market board. If you craft them yourself, your profit margins might be higher (check the [Recipe Analyzer](../analyzer/recipe.md) for crafting costs).
-   The "Revenue" calculation includes the expected value of random item rewards, so actual returns per turn-in may vary.
