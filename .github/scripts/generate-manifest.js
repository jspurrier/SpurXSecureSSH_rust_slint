const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const os = require('os');

// Usage: node generate-manifest.js <target> <version>
const target = process.argv[2];
const version = process.argv[3];
const workspace = process.cwd();
const tauriDir = path.join(workspace, 'src-tauri');
const targetDir = path.join(tauriDir, 'target', target, 'release', 'bundle');

if (!target || !version) {
    console.error("Usage: node generate-manifest.js <target> <version>");
    process.exit(1);
}

const keyContent = process.env.TAURI_SIGNING_PRIVATE_KEY;
if (!keyContent) {
    console.error("TAURI_SIGNING_PRIVATE_KEY not set");
    process.exit(1);
}

// Write key to temp file for signer
const keyPath = path.join(os.tmpdir(), 'tauri.key');
fs.writeFileSync(keyPath, keyContent);

console.log(`Generating manifest for ${target} v${version}`);

function findFile(dir, ext) {
    if (!fs.existsSync(dir)) return null;
    const files = fs.readdirSync(dir);
    const found = files.find(f => f.endsWith(ext));
    return found ? path.join(dir, found) : null;
}

// Identify artifact
let artifactToSign = null;
let downloadUrlExt = '';

// macOS
if (target.includes('apple-darwin')) {
    // Look for .app in bundle/macos
    const macosDir = path.join(targetDir, 'macos');
    const appName = fs.readdirSync(macosDir).find(f => f.endsWith('.app'));

    if (appName) {
        const appPath = path.join(macosDir, appName);
        const tarName = `${appName}.tar.gz`;
        const tarPath = path.join(macosDir, tarName);
        // Determine command based on OS running the script (GitHub Action runner)
        // macOS runner has tar
        console.log(`Compressing ${appName} to ${tarName}...`);
        execSync(`tar -czf "${tarPath}" -C "${macosDir}" "${appName}"`);
        artifactToSign = tarPath;
        downloadUrlExt = 'app.tar.gz';
    }
} else if (target.includes('windows')) {
    // Look for .exe in bundle/nsis
    const nsisDir = path.join(targetDir, 'nsis');
    const exePath = findFile(nsisDir, '.exe');
    if (exePath) {
        // For NSIS, we usually zip it for updater, OR updater supports .exe directly in v2?
        // Recommendation is .nsis.zip
        const zipName = path.basename(exePath) + '.zip';
        const zipPath = path.join(nsisDir, zipName);
        console.log(`Compressing ${path.basename(exePath)} to ${zipName}...`);
        // Use powershell to zip on windows runner
        execSync(`powershell Compress-Archive -Path "${exePath}" -DestinationPath "${zipPath}" -Force`);
        artifactToSign = zipPath;
        downloadUrlExt = 'nsis.zip';
    }
} else if (target.includes('linux')) {
    // AppImage
    const appImageDir = path.join(targetDir, 'appimage');
    const appPath = findFile(appImageDir, '.AppImage');
    if (appPath) {
        const tarName = path.basename(appPath) + '.tar.gz';
        const tarPath = path.join(appImageDir, tarName);
        console.log(`Compressing ${path.basename(appPath)} to ${tarName}...`);
        execSync(`tar -czf "${tarPath}" -C "${appImageDir}" "${path.basename(appPath)}"`);
        artifactToSign = tarPath;
        downloadUrlExt = 'AppImage.tar.gz';
    }
}

if (!artifactToSign || !fs.existsSync(artifactToSign)) {
    console.error("Could not find or create artifact to sign");
    // List directories to help debug
    try { execSync(`ls -R ${targetDir}`, { stdio: 'inherit' }); } catch { }
    process.exit(1);
}

console.log(`Signing ${artifactToSign}...`);
// Use tauri signer
// npx tauri signer sign -k keyPath artifact
const cmd = `npx tauri signer sign -k "${keyPath}" "${artifactToSign}"`;
const output = execSync(cmd, { encoding: 'utf8' });
console.log("Signer output:", output);

// Extract signature
// Output format usually: "Signature: ...string..."
// Or it writes to .sig file?
// Tauri v2 signer usually outputs a .sig file AND prints to stdout.
// We need the string for the JSON.

const sigFile = artifactToSign + '.sig';
if (!fs.existsSync(sigFile)) {
    console.error("Signature file not generated: " + sigFile);
    process.exit(1);
}
const signature = fs.readFileSync(sigFile, 'utf8').trim();

// Date
const date = new Date().toISOString();

// Construct JSON
// Note: The URL must be the public URL where this will be hosted.
// We are hosting on GitHub Releases.
// URL pattern: https://github.com/jspurrier/SpurXSecureSSH_rust_tauri/releases/download/v${version}/${filename}
const filename = path.basename(artifactToSign);
const url = `https://github.com/jspurrier/SpurXSecureSSH_rust_tauri/releases/download/v${version}/${encodeURIComponent(filename)}`;

const manifest = {
    version: version,
    notes: `Update to v${version}`,
    pub_date: date,
    platforms: {
        [target]: {
            signature: signature,
            url: url
        }
    }
};

// Check if this keys off 'target' or something else?
// v1.5.7 tauri.conf.json used {{target}}.
// Windows usage might require different key like "windows-x86_64"?
// But tauri.conf.json endpoints has {{target}}. The app will request `latest-TARGET.json`.
// The JSON CONTENT key should match the platform checking?
// The updater usually checks `platforms[current_target]`.
// So using the full target triple as key is correct if the app uses that.
// Tauri logic: `tauri::updater::...`
// Standard is: `linux-x86_64`, `darwin-aarch64`, `windows-x86_64`.
// BUT `target` variable in `release.yml` is `aarch64-apple-darwin`.
// Use mapping.

let platformKey = target;
if (target.includes('apple-darwin')) {
    platformKey = target.includes('aarch64') ? 'darwin-aarch64' : 'darwin-x86_64';
} else if (target.includes('windows')) {
    platformKey = 'windows-x86_64';
} else if (target.includes('linux')) {
    platformKey = 'linux-x86_64';
}

const finalManifest = {
    version: version,
    notes: `Update to v${version}`,
    pub_date: date,
    platforms: {
        [platformKey]: {
            signature: signature,
            url: url
        }
    }
};

const jsonPath = path.join(process.cwd(), `latest-${target}.json`);
fs.writeFileSync(jsonPath, JSON.stringify(finalManifest, null, 2));

console.log(`Generated manifest at ${jsonPath}`);
fs.unlinkSync(keyPath);
