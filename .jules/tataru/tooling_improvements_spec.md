# Tooling Improvements Specification

## 1. The Scion's Ledger (Portfolio Manager)

### Problem
Users buy items and forget about them, or forget how much they paid.

### Solution
A persistent inventory tracker.

#### Features
*   **Input**: "I bought 99x Coke at 200g on Balmung".
*   **Tracking**:
    *   Current Price on Home World.
    *   Current Profit (Unrealized).
    *   Break-even price (including tax).
*   **Alerts**:
    *   "You have been undercut!"
    *   "Price has spiked! Sell now!"

## 2. Market Arbitrage Route Planner

### Problem
Buying 10 different items requires hopping between 5 different worlds. It's inefficient to go Balmung -> Mateus -> Balmung -> Zalera.

### Solution
A Traveling Salesman Solver for Shopping Lists.

#### Workflow
1.  User adds items to "Shopping List" from Flip Finder.
2.  System groups items by Cheapest World.
3.  System orders the worlds to minimize travel time (or load screens).
4.  Output: "1. Go to Balmung: Buy X, Y. 2. Go to Mateus: Buy Z."

## 3. Retainer Venture Optimizer

### Problem
Retainers are idle or running inefficient ventures.

### Solution
Calculate the "Gil per Hour" of every Quick Venture and Targeted Venture based on current market prices.
*   Input: Retainer Level, Class, Gear Stats.
*   Output: Best venture to run right now.
