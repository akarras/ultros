// Apply saved preferences before CSS paints. Rust canonicalizes and persists
// the same aliases during hydration (global_state/theme.rs).
(function () {
    function stored(key, cookie) {
        try {
            var value = localStorage.getItem(key);
            if (value !== null) return value;
        } catch (_) {}
        try {
            var match = document.cookie.match(new RegExp('(?:^|;\\s*)' + cookie + '=([^;]*)'));
            if (match) return decodeURIComponent(match[1]);
        } catch (_) {}
        return null;
    }
    var mode = (stored('theme.mode', 'theme_mode') || 'dark').toLowerCase();
    if (mode === 'system') {
        mode = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    document.documentElement.setAttribute('data-theme', mode === 'light' ? 'light' : 'dark');
    var palette = (stored('theme.palette', 'theme_palette') || 'ultros').toLowerCase();
    switch (palette) {
        case 'violet': palette = 'ultros'; break;
        case 'emerald': palette = 'twin-adder'; break;
        case 'rose': palette = 'ascian'; break;
        case 'sky': palette = 'ishgard'; break;
        case 'teal': palette = 'limsa'; break;
        case 'amber': palette = 'uldah'; break;
        case 'ultros': case 'maelstrom': case 'twin-adder': case 'ascian':
        case 'ishgard': case 'crystarium': case 'sharlayan': case 'tuliyollal':
        case 'immortal-flames': case 'uldah': case 'limsa': case 'garlemald': break;
        default: palette = 'ultros';
    }
    document.documentElement.setAttribute('data-palette', palette);
})();
