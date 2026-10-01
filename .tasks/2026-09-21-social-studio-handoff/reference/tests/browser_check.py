"""Visual/reference-preview checks. Requires Python Playwright and Chromium.
This tests the offline HTML, not the separate React integration components.
"""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect
import json

ROOT = Path(__file__).resolve().parents[1]
results = []

def passed(name):
    results.append(name)
    print('PASS', name)

with sync_playwright() as p:
    browser = p.chromium.launch(executable_path='/usr/bin/chromium', headless=True, args=['--no-sandbox'])
    page = browser.new_page(viewport={'width':1440, 'height':1080})
    page.set_default_timeout(4000)
    js_errors = []
    requests = []
    page.on('pageerror', lambda error: js_errors.append(str(error)))
    page.on('request', lambda request: requests.append(request.url))
    page.set_content((ROOT/'preview.html').read_text(), wait_until='load')
    expect(page.locator('article')).to_have_count(3)
    assert not page.evaluate('document.documentElement.scrollWidth > innerWidth')
    page.screenshot(path=str(ROOT/'preview-desktop.png'), full_page=True)
    passed('Desktop: 3 review cards, no horizontal overflow')
    assert page.locator('aside .brand-logo').evaluate('(img) => img.complete && img.naturalWidth === 96')
    assert page.locator('header .brand-logo').is_hidden()
    assert page.locator('.studio-shell').evaluate('(el) => getComputedStyle(el).backgroundColor') == 'rgb(13, 13, 13)'
    assert 'rgb(197, 160, 89)' in page.locator('.btn-primary').first.evaluate('(el) => getComputedStyle(el).backgroundImage')
    assert page.locator('.btn-primary').first.evaluate('(el) => getComputedStyle(el).color') == 'rgb(36, 26, 18)'
    passed('Compiled brand: neutral black, gold actions with dark text, authentic sidebar logo')

    page.locator('[data-action="approve"][data-id="1"]').click()
    expect(page.locator('article')).to_have_count(2)
    page.locator('[data-filter="scheduled"]').click()
    expect(page.locator('article')).to_have_count(3)
    passed('Approval moves one clip into scheduled; only after save')

    page.locator('[data-filter="all"]').click()
    page.locator('#search').fill('Hook')
    expect(page.locator('article')).to_have_count(1)
    page.locator('#search').fill('no such clip 92347')
    expect(page.get_by_text('Keine Clips gefunden', exact=True)).to_be_visible()
    passed('Search and empty-state handling')
    page.locator('#search').fill('')
    page.locator('[data-layout="grid"]').click()
    expect(page.locator('#queue-list')).to_have_class('grid-view')
    page.locator('[data-layout="list"]').click()
    passed('Card-grid and list views')

    page.locator('[data-action="menu"][data-id="2"]').click()
    expect(page.locator('#clip-menu')).to_be_visible()
    page.get_by_role('button', name='Transkript bearbeiten', exact=True).click()
    expect(page.locator('#dialog')).to_be_visible()
    assert page.evaluate('document.querySelector("#dialog").contains(document.activeElement)')
    for _ in range(9):
        page.keyboard.press('Tab')
        assert page.evaluate('document.querySelector("#dialog").contains(document.activeElement)')
    page.keyboard.press('Escape')
    expect(page.locator('#dialog')).not_to_be_visible()
    expect(page.locator('#clip-more-2')).to_be_focused()
    passed('Dialog contains focus; Escape returns focus to trigger')

    page.locator('#tab-accounts').click()
    page.locator('[data-action="fail-next"]').click()
    page.locator('#tab-queue').click()
    page.locator('[data-filter="review"]').click()
    page.locator('[data-action="approve"][data-id="2"]').click()
    expect(page.locator('[data-clip="2"] [role="alert"]')).to_contain_text('nicht gespeichert')
    expect(page.locator('[data-clip="2"]')).to_have_attribute('data-status','review')
    passed('Failed approval retains the clip and displays a local error')

    page.locator('#tab-autopilot').click()
    field = page.locator('[name="youtube.posts_per_week"]')
    field.fill('71')
    page.locator('#schedule-form button[type="submit"]').click()
    expect(field).to_have_attribute('aria-invalid','true')
    expect(field).to_be_focused()
    field.fill('5')
    page.locator('#schedule-form button[type="submit"]').click()
    expect(page.locator('#save-status')).to_contain_text('Gespeichert')
    passed('Autopilot: invalid numbers blocked, valid edits saved')

    page.locator('#tab-accounts').click()
    page.locator('[data-action="fail-next"]').click()
    page.locator('#tab-autopilot').click()
    page.locator('[name="youtube.posts_per_week"]').fill('6')
    page.locator('#schedule-form button[type="submit"]').click()
    expect(page.locator('#save-status')).to_contain_text('nicht gespeichert')
    expect(page.locator('[name="youtube.posts_per_week"]')).to_have_value('6')
    page.locator('[data-action="reset-schedule"]').click()
    expect(page.locator('[name="youtube.posts_per_week"]')).to_have_value('5')
    passed('Failed schedule save retains draft; reset restores last successful state')
    page.screenshot(path=str(ROOT/'preview-autopilot.png'), full_page=True)

    page.locator('#tab-queue').focus()
    page.keyboard.press('ArrowRight')
    expect(page.locator('#tab-autopilot')).to_be_focused()
    page.keyboard.press('End')
    expect(page.locator('#tab-accounts')).to_be_focused()
    page.keyboard.press('Home')
    expect(page.locator('#tab-queue')).to_be_focused()
    passed('Tabs: arrows, Home and End')

    page.locator('#tab-templates').click()
    page.locator('[data-action="template"]').first.click()
    page.locator('#position-slider').fill('70')
    page.locator('#position-slider').dispatch_event('input')
    page.screenshot(path=str(ROOT/'preview-editor.png'), full_page=True)
    page.locator('#layout-form button[type="submit"]').click()
    expect(page.locator('#dialog')).not_to_be_visible()
    page.locator('[data-action="template"]').first.click()
    expect(page.locator('#position-slider')).to_have_value('70')
    page.keyboard.press('Escape')
    passed('Focused layout demo opens, saves local settings and restores them')

    # Smaller layouts must scroll internally, not overflow the document.
    for width in [390, 320]:
        page.set_viewport_size({'width':width,'height':844})
        assert page.locator('header .brand-logo').is_visible()
        for tab in ['queue','autopilot','templates','accounts']:
            page.locator('#tab-'+tab).click()
            assert not page.evaluate('document.documentElement.scrollWidth > innerWidth'), (width,tab)
        page.locator('#tab-queue').click()
        if width == 390:
            page.screenshot(path=str(ROOT/'preview-mobile.png'), full_page=True)
    passed('All four areas fit 390px and 320px without document overflow')

    assert not js_errors, js_errors
    assert not requests, requests
    passed('No JavaScript errors, API calls or network requests in the standalone preview')
    browser.close()

(ROOT/'tests/browser-results.json').write_text(json.dumps({'passed':len(results),'checks':results,'target':'preview.html only', 'font_rendering':'standalone preview uses the system fallback; product stylesheet references existing self-hosted Manrope and Sora'},ensure_ascii=False,indent=2)+'\n')
