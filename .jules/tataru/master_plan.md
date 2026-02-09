# Tataru's Grand Plan for Infinite Wealth

> "We're going to make so much gil, even Rowena will be jealous!"

## Overview
This document outlines the strategic roadmap for upgrading the Ultros investment tools. The goal is to move beyond simple "buy low, sell high" advice and provide sophisticated, data-driven tools that maximize **capital efficiency** and **velocity of money**.

## Core Pillars

### 1. Advanced Investment Mathematics
We need to stop looking at just "Profit" and "ROI". A 100% ROI item that sells once a year is a trap. We need to prioritize **Velocity**.
*   **Action**: Implement "Profit Per Day" and "Capital Velocity" metrics.
*   **Spec**: See `investment_math_spec.md`.

### 2. Portfolio Management ("The Scion's Ledger")
Investors need to track what they bought, when, and for how much.
*   **Action**: Build a portfolio tracker that alerts users when to undercut or hold.
*   **Spec**: See `tooling_improvements_spec.md`.

### 3. Route Optimization
Time is money. Teleporting costs money.
*   **Action**: Build a "Shopping Route Planner" that optimizes the path between worlds for multiple items.
*   **Spec**: See `tooling_improvements_spec.md`.

## Immediate Execution Plan
1.  Add "Potential Profit/Day" to the existing Flip Finder (`analyzer.rs`).
2.  Refine the ROI calculation to be less conservative but risk-aware.
