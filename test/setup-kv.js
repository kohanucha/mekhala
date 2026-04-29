import { getPublicKey } from "nostr-tools/pure";
import { execSync } from "child_process";

const setup = (username, secretByte) => {
    const sk = new Uint8Array(32).fill(secretByte);
    const pkRaw = getPublicKey(sk);
    const pk = typeof pkRaw === 'string' ? pkRaw : Array.from(pkRaw).map(b => b.toString(16).padStart(2, '0')).join('');
    const secretHex = Array.from(sk).map(b => b.toString(16).padStart(2, '0')).join('');
    const nwcUri = `nostr+walletconnect://${pk}?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=${secretHex}`;
    
    console.log(`Setting up KV for ${username}...`);
    execSync(`npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config ../wrangler.toml "${username}" "${nwcUri}"`, { stdio: 'inherit' });
};

setup("testuser_relay", 1);
setup("offline_user", 2);
