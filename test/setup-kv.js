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
    const config = process.env.WRANGLER_CONFIG || "../wrangler.toml";
    execSync(`npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config ${config} "${username}" "${nwcUri}"`, { stdio: 'inherit' });
};

setup("testuser_relay", 1, 3);
setup("offline_user", 2, 4);
