// Developer tool only: generate an offline catalogue from versioned game-data facts.
// No descriptions, images, player data, prices, or telemetry are imported.
// node scripts/refresh-content.mjs --version 4.10.2-LIVE.12881860
// --input-dir reads previously downloaded page-N.json and vehicles-N.json files.
import { readFile, writeFile, rename } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { createHash } from 'node:crypto';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const option = flag => args.includes(flag) ? args[args.indexOf(flag) + 1] : undefined;
const input = option('--input-dir');
const api = 'https://api.star-citizen.wiki/api';
async function request(path) {
  const response = await fetch(`${api}/${path}`, { signal: AbortSignal.timeout(30000) });
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response.json();
}
const version = option('--version') ?? (await request('game-versions/default')).data.code;
if (!/^\d+\.\d+\.\d+-[A-Z]+\.\d+$/.test(version)) throw new Error('Invalid pinned game version');
async function pages(resource, prefix) {
  const rows = [];
  let expected = null;
  let last = 1;
  for (let page = 1; page <= last; page++) {
    const response = input
      ? JSON.parse(await readFile(join(input, `${prefix}-${page}.json`), 'utf8'))
      : await request(`${resource}?version=${encodeURIComponent(version)}&page%5Bsize%5D=200&page%5Bnumber%5D=${page}`);
    if (!Array.isArray(response.data) || response.meta.current_page !== page) throw new Error(`Invalid ${resource} page ${page}`);
    last = response.meta.last_page;
    if (last < 1 || last > 100 || expected !== null && expected !== response.meta.total) throw new Error('Catalogue changed during download');
    expected = response.meta.total;
    for (const row of response.data) {
      if (row.version !== version) throw new Error(`Mixed versions: ${row.version} versus ${version}`);
      rows.push(row);
    }
  }
  if (rows.length !== expected) throw new Error(`Incomplete ${resource}: ${rows.length}/${expected}`);
  return rows;
}
const rawLocations = await pages('locations', 'page');
const rawVehicles = await pages('vehicles', 'vehicles');
const overrides = JSON.parse(await readFile(join(root, 'data/location-overrides.json'), 'utf8'));
const norm = value => value.toLowerCase().replace(/['’]/g, '').replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '');
const compact = value => norm(value).replaceAll('_', '');
const systemName = value => value.replace(/ System$/, '');
const validName = value => typeof value === 'string' && value.trim() && !/^[@<]/.test(value) && !/UNINITIALIZED|UNLOCALIZED/i.test(value);
const unique = new Map();
for (const row of rawLocations) {
  if (!/^[0-9a-f-]{36}$/i.test(row.uuid) || !validName(row.name) || !['Stanton System', 'Pyro System', 'Nyx System'].includes(row.system)) continue;
  if (unique.has(row.uuid) && (unique.get(row.uuid).name !== row.name || unique.get(row.uuid).system !== row.system)) throw new Error(`Conflicting location UUID ${row.uuid}`);
  unique.set(row.uuid, row);
}
const byNames = new Map();
for (const row of unique.values()) {
  const key = norm(row.name);
  if (!byNames.has(key)) byNames.set(key, []);
  byNames.get(key).push(row);
}
const records = new Map();
const proposals = new Map();
function propose(alias, id) {
  if (!alias || !norm(alias)) return;
  const key = norm(alias);
  if (!proposals.has(key)) proposals.set(key, new Map());
  proposals.get(key).set(id, alias);
}
for (const row of unique.values()) {
  const system = systemName(row.system);
  const sharedName = byNames.get(norm(row.name)).length > 1;
  const parent = validName(row.parent?.name) ? row.parent.name : '';
  const name = sharedName && parent && compact(parent) !== compact(row.name) ? `${row.name} (${parent})` : row.name;
  const scopes = new Set();
  let ancestor = row.parent;
  const visited = new Set([row.uuid]);
  while (ancestor && !visited.has(ancestor.uuid)) {
    visited.add(ancestor.uuid);
    const ancestorRow = unique.get(ancestor.uuid);
    if (validName(ancestor.name)) {
      scopes.add(ancestor.name);
      scopes.add(compact(ancestor.name));
      if (ancestorRow?.tag?.name && ['Star', 'Planet', 'Moon'].includes(ancestorRow.type?.classification)) scopes.add(ancestorRow.tag.name);
    }
    ancestor = ancestorRow?.parent;
  }
  if (parent) { scopes.add(`${system}_${parent}`); scopes.add(`${system}_${compact(parent)}`); }
  // A system name alone is only an accepted scope when a source ancestor supplies it.
  records.set(row.uuid, { name, system, aliases: [], scopes: [...scopes].sort() });
  for (const alias of [row.uuid, row.name, row.slug, compact(row.name), row.designation]) propose(alias, row.uuid);
  for (const prefix of [system, parent, row.parent && unique.get(row.parent.uuid)?.tag?.name].filter(Boolean)) {
    for (const suffix of [row.name, compact(row.name)]) propose(`${prefix}_${suffix}`, row.uuid);
  }
  for (const scope of scopes) {
    for (const suffix of [row.name, compact(row.name)]) propose(`${scope}_${suffix}`, row.uuid);
  }
  // Hierarchy tags are shared with children. A celestial body's tag belongs to
  // that body; other tags are only accepted if one record owns them unambiguously.
  if (row.tag?.name) {
    const peers = [...unique.values()].filter(peer => peer.tag?.name === row.tag.name);
    const bodies = peers.filter(peer => ['Star', 'Planet', 'Moon'].includes(peer.type?.classification));
    if (peers.length === 1 || bodies.length === 1 && bodies[0].uuid === row.uuid) propose(row.tag.name, row.uuid);
  }
}
let ambiguous = 0;
for (const owners of proposals.values()) {
  if (owners.size === 1) {
    const [id, alias] = [...owners.entries()][0];
    records.get(id).aliases.push(alias);
  } else { ambiguous++; }
}
// Explicit verified aliases remain authoritative. Merge them into a named record
// where unambiguous, otherwise keep the deliberate override as its own entry.
for (const override of overrides) {
  let candidates = [...unique.values()].filter(row => norm(row.name) === norm(override.name) && systemName(row.system) === override.system);
  if (candidates.length !== 1) candidates = [...unique.values()].filter(row => systemName(row.system) === override.system && override.aliases.some(alias => compact(alias) === compact(row.name)));
  const id = candidates.length === 1 ? candidates[0].uuid : `override:${override.system}:${override.name}`;
  if (!records.has(id)) records.set(id, { name: override.name, system: override.system, aliases: [] });
  const record = records.get(id);
  for (const alias of override.aliases) {
    const key = norm(alias);
    for (const other of records.values()) other.aliases = other.aliases.filter(a => norm(a) !== key);
    record.aliases.push(alias);
  }
}
const locations = [...records.values()].filter(row => row.aliases.length).map(row => ({
  ...row, aliases: [...new Map(row.aliases.map(a => [norm(a), a])).values()].sort(),
})).sort((a,b) => `${a.system}:${a.name}:${a.aliases[0]}`.localeCompare(`${b.system}:${b.name}:${b.aliases[0]}`, 'en'));
const aliasKeys = locations.flatMap(row => row.aliases.map(norm));
if (new Set(aliasKeys).size !== aliasKeys.length) throw new Error('Alias collision');
const vehicles = [...new Map(rawVehicles.filter(row => validName(row.game_name ?? row.name) && row.class_name)
  .map(row => [row.class_name.toLowerCase(), { name: row.game_name ?? row.name, class_name: row.class_name }])).values()]
  .sort((a,b) => a.class_name.localeCompare(b.class_name, 'en'));
const facts = { locations, vehicles };
const sources = {
  game_version: version,
  retrieved_on: new Date().toISOString().slice(0,10),
  source: api,
  source_location_rows: rawLocations.length,
  unique_source_locations: new Set(rawLocations.map(row => row.uuid)).size,
  catalogued_locations: locations.length,
  source_vehicle_rows: rawVehicles.length,
  catalogued_vehicles: vehicles.length,
  skipped_ambiguous_aliases: ambiguous,
  aliases: aliasKeys.length,
  systems: Object.fromEntries(['Stanton', 'Pyro', 'Nyx'].map(system => [system, locations.filter(row => row.system === system).length])),
  facts_sha256: createHash('sha256').update(JSON.stringify(facts)).digest('hex'),
  notes: 'Versioned extracted game data plus explicit log aliases. No descriptions or imagery. Data may contain hidden or unreleased records; recognition is not a claim that every entry is currently visitable. Shared unqualified names are omitted.',
};
for (const [file, data] of [['locations.json', locations], ['vehicles.json', vehicles], ['content-source.json', sources]]) {
  const destination = join(root, 'data', file);
  await writeFile(`${destination}.tmp`, JSON.stringify(data, null, 2) + '\n');
  await rename(`${destination}.tmp`, destination);
}
console.log(JSON.stringify(sources, null, 2));
