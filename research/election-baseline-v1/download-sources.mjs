import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const dir = path.resolve('research/election-baseline-v1/sources');
fs.mkdirSync(dir, { recursive: true });
for (const doi of ['doi:10.7910/DVN/PEJ5QU', 'doi:10.7910/DVN/42MVDX']) {
  const metadataUrl = `https://dataverse.harvard.edu/api/datasets/:persistentId/?persistentId=${encodeURIComponent(doi)}`;
  const response = await fetch(metadataUrl);
  if (!response.ok) throw Error(`Metadata HTTP ${response.status}`);
  const metadata = await response.json(), version = metadata.data.latestVersion;
  const source = { doi, metadataUrl, version: version.versionNumber, minorVersion: version.versionMinorNumber,
    publicationDate: version.releaseTime, license: version.license?.name, files: [] };
  for (const item of version.files) {
    const f = item.dataFile;
    if (!/codebook|1976.*senate|1976.*president|1976.*house/.test(f.filename)) continue;
    const url = `https://dataverse.harvard.edu/api/access/datafile/${f.id}?format=original`;
    const result = await fetch(url);
    if (!result.ok) throw Error(`File HTTP ${result.status}`);
    const bytes = Buffer.from(await result.arrayBuffer());
    if (bytes.length > 15000000) throw Error('Unexpected dataset size');
    const md5 = crypto.createHash('md5').update(bytes).digest('hex');
    if (f.checksum?.type === 'MD5' && md5 !== f.checksum.value) throw Error('Published checksum mismatch');
    const originalFilename = f.originalFileName || f.filename;
    const filename = originalFilename.replace(/[^a-zA-Z0-9._-]/g, '_'), local = path.join(dir, filename);
    if (fs.existsSync(local) && !fs.readFileSync(local).equals(bytes)) throw Error('Immutable source differs');
    if (!fs.existsSync(local)) fs.writeFileSync(local, bytes, { flag: 'wx' });
    source.files.push({ id: f.id, filename, originalFilename, url, bytes: bytes.length,
      sha256: crypto.createHash('sha256').update(bytes).digest('hex'), publishedChecksum: f.checksum,
      downloadedAt: new Date().toISOString() });
    console.log(JSON.stringify({ doi, filename, bytes: bytes.length, header: bytes.toString('utf8').split('\n').slice(0, 2) }));
  }
  const filename = path.join(dir, `${doi.split('/').at(-1)}-manifest.json`);
  if (!fs.existsSync(filename)) fs.writeFileSync(filename, `${JSON.stringify(source, null, 2)}\n`, { flag: 'wx' });
}
