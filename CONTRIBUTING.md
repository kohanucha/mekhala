# Contributing to Mekhala

Thank you for your interest in improving Mekhala! We welcome contributions from the community. To maintain high code quality and security, please follow these guidelines.

## 🛠 How to Contribute

1. **Report Bugs:** If you find a bug, please open an Issue with clear steps to reproduce it.
2. **Suggest Features:** Have an idea? Open an Issue to discuss it before starting work.
3. **Submit Pull Requests:**
   - Fork the repository.
   - Create a new branch for your feature or fix.
   - **Crucial:** Ensure all tests pass by running `./scripts/test.sh`.
   - Submit a Pull Request (PR) with a clear description of your changes.

## 📜 Coding Standards

- **No panics:** Always handle errors gracefully using `try`/`catch`.
- **Security First:** Never log sensitive information or bypass security limits.
- **Strict NWC focus:** We aim to keep this relay specialized for NIP-47. General social features are out of scope.
- **File naming:** Use kebab-case for all filenames (e.g., `wallet-registry.ts`). Exceptions: NIP files follow nostr convention (`nip01.ts`).
- **Module boundaries:** Each module has `index.ts` barrel re-exporting only public API. Cross-module imports use the barrel. Intra-module imports use direct file paths.
- **Formatting:** Run `npm run format:fix` before committing. Uses Prettier (single quotes, trailing commas).

## 🧪 Testing

All contributions must pass the existing test suite:
- **Type-check:** `npx tsc --noEmit`
- **Full E2E Integration:** `./scripts/test.sh`

---
*By contributing to Mekhala, you agree that your contributions will be licensed under the project's MIT License.*
