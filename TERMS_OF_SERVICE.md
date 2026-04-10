# Maity Desktop — Terms of Service

*Last updated: 2026-04-08*
*Version: 1.0 (LEG-005 — initial)*

By installing or using Maity Desktop you agree to these terms. **If you do not agree, do not use the software.**

---

## 1. License

Maity Desktop is open-source software licensed under the **MIT License** (see [LICENSE.md](LICENSE.md)). You may use, copy, modify, and distribute the software subject to the conditions in that license. **These Terms of Service apply additionally to the use of the binary distribution and any cloud features.**

---

## 2. Acceptable use

You agree to use Maity Desktop **only for lawful purposes** and in compliance with all applicable laws, including but not limited to:

- **Recording laws**: in many jurisdictions (e.g., 11 U.S. states with two-party consent: CA, FL, IL, MD, MA, MT, NH, PA, WA, CT, MI; EU GDPR; Mexican LFPDPPP), recording a conversation without informing all participants is illegal. **You are solely responsible for obtaining consent before recording.** Maity will display a reminder before each recording but does not enforce or verify consent.
- **Data protection**: GDPR, LFPDPPP, CCPA, HIPAA (if applicable), and other privacy regulations of your jurisdiction.
- **Third-party intellectual property**: do not transcribe copyrighted material in violation of fair use / fair dealing.

You agree **not** to use Maity Desktop to:

- Record persons without their informed consent where such consent is legally required
- Process personal health information (PHI) unless you have a HIPAA Business Associate Agreement (BAA) with all relevant cloud subprocessors (Deepgram, OpenAI, etc.) — Maity does not currently sign BAAs
- Conduct surveillance, harassment, or stalking
- Store data subject to export controls (ITAR, EAR) without appropriate safeguards
- Reverse-engineer or attempt to extract API credentials of other Maity users

---

## 3. Cloud features and third-party services

Maity Desktop offers two operating modes:

- **Local mode**: all processing happens on your device. No data leaves your computer.
- **Cloud mode** (default): audio is sent to **Deepgram** for transcription and text to **OpenAI / Anthropic / Groq** for summarization. See [SUBPROCESSORS.md](docs/SUBPROCESSORS.md).

When using Cloud mode, you also agree to:

- The terms of service of each subprocessor (you must have valid API keys you obtained directly from those providers — Maity uses a BYOK model)
- The fact that data sent to those subprocessors leaves your device and is processed under their respective privacy policies
- The cost of using those subprocessors is your responsibility (Maity does not bill for cloud usage)

---

## 4. No warranty

**MAITY DESKTOP IS PROVIDED "AS IS" WITHOUT WARRANTY OF ANY KIND**, express or implied, including but not limited to the warranties of merchantability, fitness for a particular purpose, accuracy of transcriptions, completeness of summaries, or non-infringement.

The transcriptions produced by Deepgram or Whisper are **not guaranteed to be accurate**. The summaries produced by LLMs (ChatGPT, Claude, Llama, etc.) are **not guaranteed to be factual**. Do not rely on Maity output for legal, medical, financial, or safety-critical decisions without independent verification by a qualified human.

---

## 5. Limitation of liability

**IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS OF MAITY DESKTOP BE LIABLE FOR ANY CLAIM, DAMAGES, OR OTHER LIABILITY**, whether in an action of contract, tort, or otherwise, arising from, out of, or in connection with the software or its use.

This includes but is not limited to:

- Lost recordings due to crashes, hardware failure, or operating system updates
- Lost transcriptions due to network failures or cloud subprocessor outages
- Costs incurred with cloud subprocessors (Deepgram, OpenAI, Anthropic, Groq) regardless of cause
- Legal liability arising from recording without consent
- Privacy violations resulting from misconfiguration of the application
- Any consequential, incidental, indirect, special, or punitive damages

To the maximum extent permitted by applicable law, your exclusive remedy is to stop using the software and uninstall it.

---

## 6. Indemnification

You agree to indemnify, defend, and hold harmless the authors and contributors of Maity Desktop from any claims, damages, costs, or expenses (including reasonable attorneys' fees) arising from:

- Your use or misuse of Maity Desktop
- Your violation of these Terms of Service
- Your violation of any third-party rights (e.g., recording someone without consent)
- Your violation of any law or regulation

---

## 7. Updates

These Terms of Service may be updated. Material changes will be announced in-app and in the GitHub Releases changelog **at least 30 days** before they take effect. Continuing to use Maity Desktop after a change constitutes acceptance of the new terms.

---

## 8. Termination

You may stop using Maity Desktop at any time by uninstalling it and deleting `$APPDATA/com.maity.ai/`.

The authors reserve the right to release updated versions that add, remove, or modify functionality. Old versions remain under the original MIT license but may stop receiving security updates.

---

## 9. Governing law

These Terms of Service are governed by the laws of the jurisdiction where the primary author resides (Mexico City, Mexico, unless otherwise updated). Any disputes shall be resolved in the courts of that jurisdiction.

For European Union users: nothing in these terms limits your statutory consumer rights under EU consumer law.

---

## 10. Severability

If any provision of these Terms of Service is found to be unenforceable, the remaining provisions shall continue in full force and effect.

---

## 11. Contact

- **Legal questions**: legal@maity.local
- **Enterprise terms / DPA / MSA**: enterprise@maity.local
- **General support**: https://github.com/Sixale730/maity_desktop/issues

---

## Document version history

| Version | Date | Changes |
|---|---|---|
| 1.0 | 2026-04-08 | Initial public ToS (LEG-005). Compatible con PRIVACY_POLICY v2.0 (LEG-001) y SUBPROCESSORS.md (LEG-004). |

---

*By using Maity Desktop you acknowledge that you have read, understood, and agree to be bound by these Terms of Service.*
