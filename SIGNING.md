# Підпис Windows та Publisher

`ProtractorPlus.exe` містить Windows version metadata:

- CompanyName: `Kiril Shevchuk`
- ProductName: `ProtractorPlus`
- ProductVersion: `3.2.0`

Ці поля відображаються у властивостях файла, але **не є цифровим підписом**. Рядок Publisher у SmartScreen/UAC береться з Authenticode-сертифіката.

## Що потрібно

Потрібен чинний code-signing сертифікат у форматі PFX, виданий довіреним центром сертифікації на ім'я `Kiril Shevchuk` (це ім'я має бути в Subject сертифіката).

Самопідписаний сертифікат не прибере попередження на інших комп'ютерах, якщо користувачі окремо не встановлять ваш кореневий сертифікат як довірений.

## Налаштування GitHub Actions

Workflow вже вміє підписувати `.exe`, якщо у репозиторії додані два Actions secrets:

- `WINDOWS_CERTIFICATE_BASE64`
- `WINDOWS_CERTIFICATE_PASSWORD`

Перетворити PFX у Base64 у PowerShell:

```powershell
$pfx = [System.IO.File]::ReadAllBytes("Kiril-Shevchuk-CodeSigning.pfx")
[System.Convert]::ToBase64String($pfx) | Set-Clipboard
```

Далі:

1. GitHub repository → **Settings**.
2. **Secrets and variables** → **Actions**.
3. Створити secret `WINDOWS_CERTIFICATE_BASE64` і вставити Base64.
4. Створити secret `WINDOWS_CERTIFICATE_PASSWORD` із паролем PFX.
5. Запустити workflow **Build ProtractorPlus**.

Після `cargo build --release` workflow:

1. відновить PFX у тимчасовій папці runner;
2. підпише `target/release/ProtractorPlus.exe` через `SignTool` з SHA-256;
3. додасть timestamp;
4. перевірить Authenticode-підпис;
5. видалить тимчасовий PFX;
6. завантажить підписаний `.exe` як artifact.

Якщо secrets не задані, workflow продовжує працювати й створює звичайний непідписаний `.exe`.

## Перевірка локально

```powershell
Get-AuthenticodeSignature .\target\release\ProtractorPlus.exe | Format-List
```

Очікувано:

- `Status : Valid`
- Subject сертифіката містить `Kiril Shevchuk`

## SmartScreen

Навіть коректно підписана нова збірка може тимчасово показувати SmartScreen, поки файл або сертифікат не набере репутацію. Найстабільніший шлях без SmartScreen для кінцевих користувачів — розповсюдження через Microsoft Store.
