// Draws the OpenPainter app icon (design/icon/icon.svg). Run: node design/icon/make-icon.mjs,
// then render it with design/icon/render-icon.cjs.
import { writeFileSync, mkdirSync } from 'node:fs';
const OUT = new URL('./', import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });

// Preview-like palette
const C = { blue: '#1a9bff', deep: '#2a6dff', cyan: '#4fdcff', sky: '#9fd8ff', pink: '#ff96e4', lav: '#c7a2ff', sun: '#ffc24a', peach: '#ffb3a0' };

const defs = (seed = 4) => `
<defs>
  <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#eceef3"/></linearGradient>
  <filter id="tileShadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="10" stdDeviation="16" flood-color="#1b2440" flood-opacity=".14"/></filter>
  <filter id="soft" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="34"/></filter>
  <filter id="bleed" x="-10%" y="-10%" width="120%" height="120%">
    <feTurbulence type="fractalNoise" baseFrequency="0.013" numOctaves="3" seed="${seed}" result="n"/>
    <feDisplacementMap in="SourceGraphic" in2="n" scale="18" xChannelSelector="R" yChannelSelector="G"/>
  </filter>
  <!-- darker pigment pooled at the wash edge, the main watercolor tell -->
  <filter id="edge" x="-5%" y="-5%" width="110%" height="110%" color-interpolation-filters="sRGB">
    <feMorphology in="SourceAlpha" operator="erode" radius="5" result="er"/>
    <feComposite in="SourceAlpha" in2="er" operator="out" result="ring"/>
    <feGaussianBlur in="ring" stdDeviation="2.5" result="ringb"/>
    <feComposite in="SourceGraphic" in2="ringb" operator="in" result="ec"/>
    <feColorMatrix in="ec" type="matrix" values=".58 0 0 0 0  0 .52 0 0 0  0 0 .66 0 0  0 0 0 .6 0" result="dark"/>
    <feMerge><feMergeNode in="SourceGraphic"/><feMergeNode in="dark"/></feMerge>
  </filter>
  <filter id="grain" x="0" y="0" width="100%" height="100%">
    <feTurbulence type="fractalNoise" baseFrequency=".85" numOctaves="2" seed="11"/>
    <feColorMatrix type="matrix" values="0 0 0 0 .2  0 0 0 0 .25  0 0 0 0 .4  0 0 0 -1.6 1.05"/>
  </filter>
  <filter id="granule" x="0" y="0" width="100%" height="100%">
    <feTurbulence type="fractalNoise" baseFrequency=".022" numOctaves="2" seed="${seed + 3}"/>
    <feColorMatrix type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 -2.2 1.25"/>
  </filter>
  <filter id="brushShadow" x="-20%" y="-50%" width="140%" height="200%"><feDropShadow dx="6" dy="10" stdDeviation="8" flood-color="#25202b" flood-opacity=".2"/></filter>
</defs>`;

const tile = (fill = 'url(#tile)') => `<rect x="100" y="100" width="824" height="824" rx="185" fill="${fill}" filter="url(#tileShadow)"/>
<rect x="101.5" y="101.5" width="821" height="821" rx="184" fill="none" stroke="#000" stroke-opacity=".06" stroke-width="3"/>`;

// The watercolor wash: soft color blobs, bled edges, pooled edge pigment, paper grain.
let washN = 0;
function wash({ x = 222, y = 222, w = 580, h = 580, r = 70, shape = 'rect', palette = 'preview', organic = false } = {}) {
  const id = `w${washN++}`;
  const P = palette === 'preview'
    ? [[C.sky, x + w * .5, y + h * .5, w * .7], [C.blue, x + w * .2, y + h * .15, w * .42], [C.cyan, x + w * .78, y + h * .2, w * .38], [C.deep, x + w * .45, y + h * .02, w * .25],
       [C.blue, x + w * .85, y + h * .55, w * .3], [C.lav, x + w * .5, y + h * .72, w * .24], [C.pink, x + w * .18, y + h * .72, w * .26], [C.pink, x + w * .7, y + h * .9, w * .16], [C.sun, x + w * .12, y + h * 1.02, w * .26]]
    : palette.blobs.map(([c, fx, fy, fr]) => [c, x + w * fx, y + h * fy, w * fr]);
  const base = palette === 'preview' ? C.sky : palette.base;
  const blobs = P.map(([c, cx, cy, rr]) => `<ellipse cx="${cx}" cy="${cy}" rx="${rr}" ry="${rr * .8}" fill="${c}"/>`).join('');
  const clipShape = shape === 'rect'
    ? `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${r}"/>`
    : shape; // custom path
  return `<mask id="${id}c" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024"><g fill="#fff" filter="url(#${organic ? 'bloom' : 'bleed'})">${clipShape}</g></mask>
<g filter="url(#edge)"><g mask="url(#${id}c)">
  <rect x="${x - 60}" y="${y - 60}" width="${w + 120}" height="${h + 120}" fill="${base}"/>
  <g filter="url(#soft)">${blobs}</g>
  <rect x="${x - 60}" y="${y - 60}" width="${w + 120}" height="${h + 120}" filter="url(#granule)" opacity=".13"/>
  <rect x="${x - 60}" y="${y - 60}" width="${w + 120}" height="${h + 120}" filter="url(#grain)" opacity=".18"/>
</g></g>`;
}

// A round artist brush lying along +x from its handle end. len is the whole length.
let brushN = 0;
function brush({ x, y, angle, len = 820, handle = 'lacquer', paint = C.blue, w = 1, flip = false } = {}) {
  const id = `b${brushN++}`, s = len / 640;
  const H = {
    lacquer: ['#141519', '#4a4c55', '#1d1e23', '#0b0b0e'],
    wood: ['#7a4a22', '#d79a5b', '#a8682f', '#5a3214'],
    red: ['#7d1420', '#ff5a64', '#c8283a', '#5c0c14'],
    white: ['#b9bfca', '#ffffff', '#e4e8ef', '#a0a7b4'],
    blue: ['#0d3f9e', '#5fa8ff', '#1f63d8', '#0a2c70'],
  }[handle];
  const hw = 15 * w;
  return `<g filter="url(#brushShadow)"><g transform="translate(${x} ${y}) rotate(${angle}) scale(${s} ${flip ? -s : s})">
  <linearGradient id="${id}h" x1="0" y1="${-hw}" x2="0" y2="${hw}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${H[0]}"/><stop offset=".28" stop-color="${H[1]}"/><stop offset=".55" stop-color="${H[2]}"/><stop offset="1" stop-color="${H[3]}"/></linearGradient>
  <linearGradient id="${id}f" x1="0" y1="${-hw}" x2="0" y2="${hw}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#7d828b"/><stop offset=".3" stop-color="#f6f8fb"/><stop offset=".55" stop-color="#bfc4cb"/><stop offset="1" stop-color="#5f646c"/></linearGradient>
  <linearGradient id="${id}t" x1="490" y1="0" x2="640" y2="0" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#e2c79c"/><stop offset=".24" stop-color="#d4b07a"/><stop offset=".38" stop-color="${paint}"/><stop offset="1" stop-color="${paint}"/></linearGradient>
  <linearGradient id="${id}ts" x1="0" y1="${-hw}" x2="0" y2="${hw}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#000" stop-opacity=".2"/><stop offset=".3" stop-color="#fff" stop-opacity=".12"/><stop offset=".6" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".2"/></linearGradient>
  <path d="M8 ${-8 * w} C 120 ${-11 * w} 260 ${-15 * w} 335 ${-15 * w} L 402 ${-13 * w} L 402 ${13 * w} L 335 ${15 * w} C 260 ${15 * w} 120 ${11 * w} 8 ${8 * w} Q 0 0 8 ${-8 * w} Z" fill="url(#${id}h)"/>
  <path d="M400 ${-14 * w} L 492 ${-12 * w} L 492 ${12 * w} L 400 ${14 * w} Z" fill="url(#${id}f)"/>
  <path d="M412 ${-13.7 * w} V ${13.7 * w} M420 ${-13.5 * w} V ${13.5 * w}" stroke="#5c6168" stroke-opacity=".55" stroke-width="2.4"/>
  <path d="M490 ${-12 * w} C 520 ${-17 * w} 560 ${-17 * w} 590 ${-10 * w} Q 625 ${-3 * w} 644 0 Q 625 ${3 * w} 590 ${10 * w} C 560 ${17 * w} 520 ${17 * w} 490 ${12 * w} Z" fill="url(#${id}t)"/>
  <path d="M490 ${-12 * w} C 520 ${-17 * w} 560 ${-17 * w} 590 ${-10 * w} Q 625 ${-3 * w} 644 0 Q 625 ${3 * w} 590 ${10 * w} C 560 ${17 * w} 520 ${17 * w} 490 ${12 * w} Z" fill="url(#${id}ts)"/>
  <path d="M512 ${-7 * w} Q 570 ${-6 * w} 628 ${-1 * w} M512 0 Q 575 0 636 0 M512 ${7 * w} Q 570 ${6 * w} 628 ${1 * w}" stroke="#3a1f10" stroke-opacity=".15" stroke-width=".9" fill="none" stroke-linecap="round"/>
  <path d="M60 ${-6 * w} C 160 ${-8 * w} 280 ${-10 * w} 390 ${-9 * w}" stroke="#fff" stroke-opacity=".14" stroke-width="2" fill="none" stroke-linecap="round"/>
</g></g>`;
}

// A clear glass water jar seen from the front. Returns back, water and front layers,
// so the brush can be drawn between them and look like it stands in the water.
let jarN = 0;
function jar({ cx, top, w = 240, h = 250, water = C.cyan, water2 = C.blue, tint = .55, rip = null } = {}) {
  const id = `j${jarN++}`, l = cx - w / 2, r = cx + w / 2, b = top + h, rim = 18, wl = top + h * .32;
  const body = `M${l} ${top} L ${l + 4} ${b - 40} Q ${l + 6} ${b} ${l + 46} ${b} L ${r - 46} ${b} Q ${r - 6} ${b} ${r - 4} ${b - 40} L ${r} ${top}`;
  return {
    back: `<linearGradient id="${id}w" x1="0" y1="${wl}" x2="0" y2="${b}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${water}" stop-opacity="${tint}"/><stop offset="1" stop-color="${water2}" stop-opacity="${tint + .2}"/></linearGradient>
  <ellipse cx="${cx}" cy="${b + 6}" rx="${w * .55}" ry="22" fill="#0b1a3a" opacity=".22" filter="url(#smallBlur)"/>
  <path d="${body} Z" fill="#ffffff" fill-opacity=".38"/>
  <ellipse cx="${cx}" cy="${top}" rx="${w / 2}" ry="${rim}" fill="#e9f4ff" fill-opacity=".6" stroke="#fff" stroke-width="5"/>`,
    water: `<path d="M${l + 3} ${wl} Q ${cx} ${wl + 26} ${r - 3} ${wl} L ${r - 4} ${b - 40} Q ${r - 6} ${b - 2} ${r - 46} ${b - 2} L ${l + 46} ${b - 2} Q ${l + 6} ${b - 2} ${l + 4} ${b - 40} Z" fill="url(#${id}w)"/>
  <ellipse cx="${cx}" cy="${wl}" rx="${w / 2 - 3}" ry="${rim - 4}" fill="${water}" fill-opacity=".45" stroke="#fff" stroke-opacity=".7" stroke-width="3"/>
  ${rip === null ? '' : `<ellipse cx="${cx + rip}" cy="${wl + 2}" rx="30" ry="7" fill="none" stroke="#fff" stroke-opacity=".8" stroke-width="3"/><ellipse cx="${cx + rip}" cy="${wl + 2}" rx="52" ry="11" fill="none" stroke="#fff" stroke-opacity=".4" stroke-width="2.5"/>`}`,
    front: `<path d="${body}" fill="none" stroke="#ffffff" stroke-width="7" stroke-linejoin="round"/>
  <path d="${body}" fill="none" stroke="#7d9cc4" stroke-opacity=".35" stroke-width="2"/>
  <path d="M${l + 22} ${top + 34} L ${l + 25} ${b - 50}" stroke="#fff" stroke-opacity=".85" stroke-width="12" stroke-linecap="round"/>
  <path d="M${r - 26} ${top + 50} L ${r - 28} ${top + 110}" stroke="#fff" stroke-opacity=".7" stroke-width="7" stroke-linecap="round"/>
  <path d="M${l} ${top} A ${w / 2} ${rim} 0 0 0 ${r} ${top}" fill="none" stroke="#fff" stroke-width="7"/>`,
  };
}

const svg = (body, seed) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">${defs(seed)}${body}</svg>`;

// New palettes. Each keeps the soft many-color blend but uses other hues and blends in another direction.
const PAL = {
  // warm top right flowing to violet bottom left
  sunset: { base: '#ff9a7a', blobs: [['#ff6a4d', .85, .1, .45], ['#ffb23f', .55, .05, .32], ['#ff4f8b', .5, .5, .38], ['#ff8a5c', .9, .55, .3], ['#8b5cff', .12, .85, .4], ['#b45cff', .4, .9, .28], ['#ffd36b', 1, -.05, .22], ['#3fb8ff', -.05, 1.05, .18]] },
  // teal and mint from bottom right, indigo top left
  lagoon: { base: '#4fd6c4', blobs: [['#14b8a6', .85, .9, .45], ['#7ef0c8', .55, .65, .32], ['#3f4fe0', .1, .1, .42], ['#6a7cff', .35, .2, .28], ['#c8ef5a', 1, .5, .22], ['#2fc4ff', .2, .6, .3], ['#a3f7e2', .7, .3, .2]] },
  // left to right bands: magenta, plum, peach, sun
  orchard: { base: '#ff9fb8', blobs: [['#e43f9a', .05, .45, .38], ['#7a3fd2', .3, .2, .3], ['#7a3fd2', .25, .85, .28], ['#ff7aa8', .5, .55, .3], ['#ffae74', .75, .35, .3], ['#ffe066', .98, .7, .32], ['#ffd08a', .85, .05, .2]] },
  // night top, glowing green and aqua lower
  aurora: { base: '#3b6fd8', blobs: [['#3a1fa8', .4, -.05, .5], ['#6a3df0', .85, .15, .3], ['#1fd39a', .25, .65, .34], ['#45e0ff', .7, .62, .32], ['#c86bff', .9, .95, .26], ['#9cf5c9', .45, .9, .24]] },
  // lime, lemon and orange, a little sky in one corner
  citrus: { base: '#ffd24a', blobs: [['#ff8a2a', .1, .2, .38], ['#ffe14d', .55, .3, .36], ['#8fe04a', .85, .75, .38], ['#ff5a3a', .25, .55, .22], ['#c9f26a', .5, .85, .28], ['#58c8ff', .98, .02, .18]] },
  // raspberry and ink blue, crossing on the other diagonal
  berry: { base: '#c45cff', blobs: [['#ff2f6d', .1, .1, .42], ['#ff7ab0', .45, .35, .26], ['#2f3bff', .9, .9, .42], ['#4a8bff', .6, .7, .3], ['#ff9a3d', .95, .1, .2], ['#7a2fd8', .2, .9, .3]] },
};
// Big brush that crosses the tile from top right to bottom left, both ends past the tile edge.
const bigBrush = (o = {}) => brush({ x: 812, y: 26, angle: 117, len: 1010, w: 1.3, ...o });
const mirrorBrush = (o = {}) => brush({ x: 212, y: 26, angle: 63, len: 1010, w: 1.3, ...o });
// Final: Orchard wash, wooden brush, magenta tip. The brush is clipped to the tile so
// macOS 26+ does not shrink the icon onto a gray tile.
const clip = body => `<clipPath id="tileclip"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath><g clip-path="url(#tileclip)">${body}</g>`;
const WASH = `<path d="M292 226 C 380 214 460 236 560 222 C 650 210 720 230 770 226 Q 804 230 806 268 C 812 360 794 450 808 540 C 820 640 798 700 802 760 Q 800 798 762 800 C 680 812 600 790 500 806 C 420 818 340 794 268 802 Q 226 804 222 762 C 214 680 236 600 222 500 C 210 410 232 330 224 270 Q 226 228 292 226 Z"/>`;
const icons = {
  'icon': [PAL.orchard, clip(brush({ x: 818, y: -50, angle: 117, len: 1010, w: 1.3, handle: 'wood', paint: '#e43f9a', flip: true })), 33],
};
for (const [k, [pal, br, seed]] of Object.entries(icons)) writeFileSync(OUT + k + '.svg', svg(tile() + wash({ palette: pal, shape: WASH }) + br, seed));
console.log(Object.keys(icons).join(' '));
