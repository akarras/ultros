from playwright.sync_api import Page, expect, sync_playwright
import time

def verify_homepage(page: Page):
    # Go to the homepage
    page.goto("http://localhost:3000")

    # Wait for the title to be visible (ensures page load)
    # The title text is "Ultros" inside the h1
    expect(page.get_by_text("Ultros", exact=True).first).to_be_visible(timeout=30000)

    # Wait a bit for animations/rendering
    time.sleep(2)

    # Check for Feature Cards
    # We expect "Item Explorer" to be visible
    expect(page.get_by_text("Item Explorer")).to_be_visible()

    # Check that "Market Trends" is visible
    expect(page.get_by_text("Market Trends")).to_be_visible()

    # Take a screenshot
    page.screenshot(path="/home/jules/verification/homepage_design.png", full_page=True)

if __name__ == "__main__":
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page()
        try:
            verify_homepage(page)
        except Exception as e:
            print(f"Error: {e}")
        finally:
            browser.close()
