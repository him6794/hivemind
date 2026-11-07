// Legal text for the website: terms of service and privacy policy, in Chinese and English.
export const LEGAL_COPY = {
  zh: {
    terms: {
      title: `用戶條款`,
      intro: `歡迎使用 Hivemind。使用本網站、Master、Worker 或其他 Hivemind 服務，即表示你同意以下條款。如果你不同意，請不要使用本服務。`,
      updated: `生效日期：2026 年 10 月 30 日`,
      sections: [
        {
          heading: `1. 服務內容`,
          paragraphs: [
            `Hivemind 讓使用者把工作送到分散式網路中的電腦上執行，並依已確認的工作量計費。`,
            `首次公開版本只提供 64 位元 Windows 用戶端。功能、可用性與限制可能隨版本調整。`,
          ],
        },
        {
          heading: `2. 帳號`,
          paragraphs: [
            `使用服務前需要註冊帳號。你應提供正確的使用者名稱，並妥善保管密碼。`,
            `你對使用你帳號所進行的活動負責。發現帳號遭他人使用時，請立即通知我們。未滿 18 歲者應先取得法定代理人同意。`,
          ],
        },
        {
          heading: `3. CPT 額度`,
          paragraphs: [
            `CPT 是服務內部的工作額度，不是貨幣，不能兌換、轉讓或提領。`,
            `分享電腦所得的額度只能用於你自己的工作，不是收入，也不是任何投資或報酬承諾。`,
          ],
        },
        {
          heading: `4. 費用與結算`,
          paragraphs: [
            `送出工作時設定的最高額度，是該工作的收費上限，已包含平台費。`,
            `系統只對已由多台電腦比對確認的工作收費，依確認的用量計算，並加收 10% 平台費。未達成確認的工作不收費，未使用的額度會退回你的帳戶。`,
            `已確認的工作費用原則上不退還，但法律另有規定，或因服務問題無法完成工作的情況除外，我們會依適用法律處理。`,
          ],
        },
        {
          heading: `5. 分享你的電腦`,
          paragraphs: [
            `執行 Worker 時，你的電腦可能會執行其他使用者的工作。工作以 hcode 執行，無法存取你電腦上的檔案、網路或其他程式。`,
            `你可以隨時停止分享，停止後不會再接收新的工作。你應確保你的電腦、網路連線與使用權限符合本條款與適用法律。`,
          ],
        },
        {
          heading: `6. 可接受的使用`,
          paragraphs: [
            `你不得利用本服務：送出違法內容或惡意程式；攻擊、入侵或干擾本服務或其他人的電腦；繞過額度、速率、沙箱或其他安全限制；冒用他人身分或以不實資料註冊；進行詐欺或規避費用。`,
            `違反本條的帳號，我們可以暫停或終止。`,
          ],
        },
        {
          heading: `7. 結果`,
          paragraphs: [
            `多台電腦的結果一致時，系統才會視為已確認的結果。確認不代表結果在技術、商業或法律上適用於你的用途。`,
            `重要的結果請你自行驗證。`,
          ],
        },
        {
          heading: `8. 服務變動與終止`,
          paragraphs: [
            `我們可能調整、暫停或終止部分或全部服務。重大變更會在網站公告，並在可行時事先通知。`,
            `你可以隨時停止使用服務，並依隱私權政策請求刪除帳號。`,
          ],
        },
        {
          heading: `9. 免責與責任限制`,
          paragraphs: [
            `在法律允許的範圍內，服務以現況提供，我們不保證服務不中斷或沒有錯誤。`,
            `在法律允許的範圍內，我們不對間接、附帶或衍生的損失負責。法律不允許排除或限制的責任，不受本條影響。`,
          ],
        },
        {
          heading: `10. 軟體授權與你的內容`,
          paragraphs: [
            `Hivemind 原始碼依 LICENSE 檔案所載的 MIT 授權條款釋出。`,
            `你送出的程式與資料仍屬於你。你授權我們為執行工作而儲存、處理，並把它們交給執行該工作的電腦。`,
          ],
        },
        {
          heading: `11. 隱私`,
          paragraphs: [`我們如何蒐集、使用與保護個人資料，依隱私權政策說明。請一併閱讀。`],
        },
        {
          heading: `12. 準據法`,
          paragraphs: [`本條款依中華民國法律解釋與適用。`],
        },
        {
          heading: `13. 條款變更`,
          paragraphs: [`我們可能更新本條款，並在網站公告生效日期。更新後繼續使用服務，即表示你接受更新後的條款。`],
        },
      ],
    },
    privacy: {
      title: `隱私權政策`,
      intro: `本政策說明 Hivemind 網站與應用程式如何蒐集、使用與保護你的個人資料，並依《個人資料保護法》處理。`,
      updated: `生效日期：2026 年 10 月 30 日`,
      sections: [
        {
          heading: `1. 我們蒐集的資料`,
          paragraphs: [
            `帳號資料：使用者名稱，以及密碼的雜湊值。系統不保存密碼明文。`,
            `額度與交易紀錄：CPT 餘額、分錄與結算紀錄。`,
            `工作資料：你送出的程式、輸入資料、執行回執、結果與用量紀錄。`,
            `Worker 與電腦資料：使用者名稱、Worker 識別碼、IP 位址、你設定的所在地點標籤（若有）、硬體資源規格（例如 CPU 執行緒數與記憶體），以及效能測試與資源觀察結果。`,
            `網路與安全紀錄：IP 位址、請求時間與速率限制的計數，用於防止濫用。`,
            `VPN 註冊資料：電腦加入覆蓋網路時建立的裝置紀錄，例如節點名稱與虛擬網路位址。`,
            `本機資料：Master 與 Worker 會在你的電腦上保存設定檔（.env）、沙箱資料、VPN 狀態與日誌，這些資料位於你的使用者資料夾中。`,
          ],
        },
        {
          heading: `2. 網站、Cookie 與第三方`,
          paragraphs: [
            `網站目前不設定 Cookie，也不使用第三方分析或廣告追蹤工具。`,
            `網站透過 Cloudflare 提供與傳輸。Cloudflare 可能處理你的 IP 位址與請求標頭，用於傳輸與安全防護。`,
          ],
        },
        {
          heading: `3. 蒐集目的`,
          paragraphs: [`提供服務、帳號管理與登入、計費與結算、客服支援、防止濫用與詐欺、系統安全與故障排除，以及遵循法律義務。`],
        },
        {
          heading: `4. 工作會送到其他人的電腦`,
          paragraphs: [
            `工作會被送到執行該工作的電腦，這些電腦可能屬於其他使用者。該電腦會處理工作的程式與輸入資料。`,
            `請不要把你不願意讓他人處理的個人資料放進工作的輸入資料中。`,
          ],
        },
        {
          heading: `5. 資料分享`,
          paragraphs: [
            `我們不會出售你的個人資料。`,
            `我們只在提供服務所必要的範圍內共享資料，對象包括：Cloudflare（網站託管與傳輸）、執行本服務的伺服器、VPN 協調服務，以及依法律要求的機關。`,
          ],
        },
        {
          heading: `6. 保存期間`,
          paragraphs: [
            `我們只在達成上述目的所需的期間內保存個人資料。`,
            `你請求刪除帳號時，我們會刪除或匿名化可刪除的資料。依法需要保存的結算與稅務紀錄，會保存至法定期限。`,
          ],
        },
        {
          heading: `7. 資料安全`,
          paragraphs: [
            `密碼以雜湊方式保存。網站與 API 以 HTTPS 傳輸，登入使用存取權杖，登入與註冊設有速率限制。`,
            `沒有任何系統能保證絕對安全。發生可能影響你的個資事故時，我們會依法律規定通知你。`,
          ],
        },
        {
          heading: `8. 你的權利`,
          paragraphs: [
            `依《個人資料保護法》，你可以請求查詢或閱覽你的個人資料、製給複本、補充或更正、停止蒐集、處理或利用，以及刪除。`,
            `我們可能會先核對你的身分，才處理你的請求。`,
          ],
        },
        {
          heading: `9. 未成年人`,
          paragraphs: [`未滿 18 歲者應先取得法定代理人同意，才能使用服務。`],
        },
        {
          heading: `10. 政策變更`,
          paragraphs: [`我們可能更新本政策，並在網站公告生效日期。重大變更會事先通知。`],
        },
      ],
    },
  },
  en: {
    terms: {
      title: `Terms of service`,
      intro: `Welcome to Hivemind. By using the website, Master, Worker, or any other Hivemind service, you agree to these terms. If you do not agree, do not use the service.`,
      updated: `Effective date: October 30, 2026`,
      sections: [
        {
          heading: `1. The service`,
          paragraphs: [
            `Hivemind lets you send tasks to computers in a distributed network and charges for confirmed work.`,
            `The first public version supports 64-bit Windows clients only. Features, availability, and limits may change between versions.`,
          ],
        },
        {
          heading: `2. Accounts`,
          paragraphs: [
            `You need an account to use the service. Give a correct username and keep your password safe.`,
            `You are responsible for activity under your account. Tell us at once if you suspect someone else is using it. If you are under 18, a parent or guardian must agree to these terms first.`,
          ],
        },
        {
          heading: `3. CPT credit`,
          paragraphs: [
            `CPT is an internal unit for tasks. It is not money and cannot be converted, transferred, or withdrawn.`,
            `Credit earned by sharing a computer can be used only for your own tasks. It is not income and not a promise of any return.`,
          ],
        },
        {
          heading: `4. Charges and settlement`,
          paragraphs: [
            `The maximum you set for a task is the most that task can cost. It already includes the platform fee.`,
            `The network charges only for work that enough matching computers confirm. The charge is the confirmed usage plus a 10% platform fee. A task without a confirmed result is not charged, and unused credit returns to your account.`,
            `Confirmed charges are generally not refundable. Where the law provides otherwise, or where a service failure prevents the work from being completed, we will handle the matter under applicable law.`,
          ],
        },
        {
          heading: `5. Sharing your computer`,
          paragraphs: [
            `When you run Worker, your computer may run tasks from other users. Tasks run in hcode, which cannot access files, the network, or other programs on your computer.`,
            `You can stop sharing at any time, and then you will not receive new tasks. You must keep your computer, its network connection, and your right to use them within these terms and applicable law.`,
          ],
        },
        {
          heading: `6. Acceptable use`,
          paragraphs: [
            `You must not use the service to send unlawful content or malware; to attack, break into, or disrupt the service or other people's computers; to bypass budgets, rate limits, the sandbox, or other safeguards; to impersonate someone or register with false information; or to commit fraud or avoid charges.`,
            `We may suspend or end accounts that break this section.`,
          ],
        },
        {
          heading: `7. Results`,
          paragraphs: [
            `A result is treated as confirmed when enough matching computers agree. Confirmation does not mean the result is correct or suitable for your purpose in technical, commercial, or legal terms.`,
            `Check important results yourself.`,
          ],
        },
        {
          heading: `8. Changes and termination`,
          paragraphs: [
            `We may change, suspend, or stop some or all of the service. We will announce material changes on the website and give advance notice where practical.`,
            `You may stop using the service at any time and ask us to delete your account, as the privacy policy describes.`,
          ],
        },
        {
          heading: `9. Disclaimers and limits of liability`,
          paragraphs: [
            `To the extent the law allows, the service is provided as is, and we do not guarantee that it will be uninterrupted or error-free.`,
            `To the extent the law allows, we are not liable for indirect, incidental, or consequential losses. Liability that the law does not allow to be limited is not affected.`,
          ],
        },
        {
          heading: `10. Software license and your content`,
          paragraphs: [
            `The Hivemind source code is released under the MIT license in the LICENSE file.`,
            `The programs and data you submit remain yours. You allow us to store and process them, and to send them to the computers that run your tasks.`,
          ],
        },
        {
          heading: `11. Privacy`,
          paragraphs: [`The privacy policy explains how we collect, use, and protect personal data. Please read it together with these terms.`],
        },
        {
          heading: `12. Governing law`,
          paragraphs: [`These terms are governed by the laws of the Republic of China (Taiwan).`],
        },
        {
          heading: `13. Changes to these terms`,
          paragraphs: [`We may update these terms and will post the effective date when we do. Continued use after an update means you accept the updated terms.`],
        },
      ],
    },
    privacy: {
      title: `Privacy policy`,
      intro: `This policy explains how the Hivemind website and apps collect, use, and protect personal data, in line with Taiwan's Personal Information Protection Act.`,
      updated: `Effective date: October 30, 2026`,
      sections: [
        {
          heading: `1. Data we collect`,
          paragraphs: [
            `Account data: your username and a hash of your password. We do not store your password in plain text.`,
            `Credit and transactions: your CPT balance, ledger entries, and settlement records.`,
            `Task data: the program you submit, its input data, the receipts that computers return, results, and usage records.`,
            `Worker and computer data: your username, the Worker identifier, your IP address, a location label if you set one, hardware resource details such as CPU threads and memory, and benchmark and resource observations.`,
            `Network and security records: IP addresses, request times, and rate-limit counters, used to prevent abuse.`,
            `VPN enrollment data: device records created when your computer joins the overlay network, such as a node name and a virtual network address.`,
            `Local data: Master and Worker keep settings (.env), sandbox files, VPN state, and logs on your computer, in your user folder.`,
          ],
        },
        {
          heading: `2. Website, cookies, and third parties`,
          paragraphs: [
            `The website does not currently set cookies and does not use third-party analytics or advertising trackers.`,
            `Cloudflare serves the website. It may process IP addresses and request headers for delivery and security.`,
          ],
        },
        {
          heading: `3. Why we use data`,
          paragraphs: [`To provide the service, manage accounts and sign-in, charge and settle work, support you, prevent abuse and fraud, keep the system secure, troubleshoot problems, and meet legal obligations.`],
        },
        {
          heading: `4. Tasks reach other people's computers`,
          paragraphs: [
            `A task is sent to a computer that runs it, and that computer may belong to another user. The computer processes the task's program and input data.`,
            `Do not put personal data in task inputs unless you are comfortable with other users' computers processing it.`,
          ],
        },
        {
          heading: `5. Who receives data`,
          paragraphs: [
            `We do not sell personal data.`,
            `We share data only as needed to run the service: Cloudflare (website hosting and delivery), the servers that run the service, the VPN coordination service, and authorities when the law requires it.`,
          ],
        },
        {
          heading: `6. Retention`,
          paragraphs: [
            `We keep personal data only as long as needed for the purposes above.`,
            `When you ask us to delete your account, we delete or anonymize the data that can be deleted. Records the law requires us to keep, such as settlement and tax records, are kept for the legal period.`,
          ],
        },
        {
          heading: `7. Security`,
          paragraphs: [
            `Passwords are stored as hashes. The website and API use HTTPS, sessions use access tokens, and sign-in and registration are rate-limited.`,
            `No system is completely secure. If a personal data incident may affect you, we will notify you as the law requires.`,
          ],
        },
        {
          heading: `8. Your rights`,
          paragraphs: [
            `Under the Personal Information Protection Act, you may ask us to access or review your data, provide a copy, correct or supplement it, stop collecting, processing, or using it, or delete it.`,
            `We may verify your identity before we act on a request.`,
          ],
        },
        {
          heading: `9. Minors`,
          paragraphs: [`If you are under 18, a parent or guardian must agree before you use the service.`],
        },
        {
          heading: `10. Changes to this policy`,
          paragraphs: [`We may update this policy and will post the effective date. We will notify you in advance of significant changes.`],
        },
      ],
    },
  },
};
