import { getPublicKey } from "nostr-tools/pure";
import { execSync } from "child_process";

const setup = (username, walletSecretByte, bridgeSecretByte) => {
    const sk = new Uint8Array(32).fill(walletSecretByte);
    const pkRaw = getPublicKey(sk);
    const pk = typeof pkRaw === 'string' ? pkRaw : Array.from(pkRaw).map(b => b.toString(16).padStart(2, '0')).join('');
    
    // Bridge secret is DIFFERENT from wallet secret
    const bridgeSk = new Uint8Array(32).fill(bridgeSecretByte);
    const secretHex = Array.from(bridgeSk).map(b => b.toString(16).padStart(2, '0')).join('');
    
    const nwcUri = `nostr+walletconnect://${pk}?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=${secretHex}`;
    
    console.log(`Setting up KV for ${username}...`);
    console.log(`  Wallet PK: ${pk}`);
    console.log(`  Bridge Secret: ${secretHex}`);
    // Drop a sealed row from an earlier run first: the store prefers
    // `lnaddr:<username>` over the plaintext key, so a leftover would shadow
    // the value seeded here and hide the migration path this suite relies on.
    try {
        execSync(`npx wrangler kv key delete --binding MEKHALA_NWC_KV --local --config ../wrangler.toml "lnaddr:${username}"`, { stdio: 'ignore' });
    } catch (e) {}
    execSync(`npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config ../wrangler.toml "${username}" "${nwcUri}"`, { stdio: 'inherit' });
};

setup("testuser_relay", 1, 3);
setup("offline_user", 2, 4);
