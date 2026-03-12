# Admin Dashboard Design Spec

## Overview

Claudeway'e optional bir admin dashboard eklemek. Svelte + Vite ile build edilen SPA, `rust-embed` ile binary'ye gömülür ve `/dashboard` path'inden serve edilir. `--features dashboard` feature flag'i ile aktifleşir.

## Hedef Kullanıcı

Sadece admin — Claudeway'i deploy eden kişi. Çoklu kullanıcı veya role-based access yok.

## Proje Yapısı

```
claudeway/
├── dashboard/                  # Svelte SPA (vanilla Svelte + Vite)
│   ├── package.json
│   ├── vite.config.ts
│   ├── src/
│   │   ├── routes/            # Sayfalar (Overview, Sessions, Logs, Keys, Cost)
│   │   ├── lib/               # Paylaşılan componentler, API client
│   │   └── app.html
│   └── dist/                  # Build output (gitignore'd)
├── src/
│   ├── dashboard.rs           # Static file serve + feature gate
│   └── handlers/
│       └── admin.rs           # Dashboard'a veri sağlayan admin API endpointleri
├── build.rs                   # dashboard/ build'ini tetikler (npm run build)
└── Cargo.toml                 # [features] dashboard = ["rust-embed"]
```

## Feature Flag

- `--features dashboard` ile optional
- Aktifken `build.rs` Svelte projesini derler, `rust-embed` ile binary'ye gömer
- Flag yoksa binary boyutuna sıfır ek
- Swagger UI pattern'i ile tutarlı (`--features swagger` gibi)

## Admin API Endpointleri

Hepsi `/admin` prefix'i altında. Sadece admin key erişebilir, diğer key'ler 403 alır.

| Endpoint | Açıklama |
|---|---|
| `POST /admin/login` | Admin key ile login, httpOnly session cookie döner |
| `GET /admin/overview` | Uptime, toplam istek, toplam maliyet, aktif session sayısı, model bazlı breakdown |
| `GET /admin/sessions` | Tüm session'ların listesi. Sayfalama: `?page=1&limit=20`. Filtreleme: `?model=sonnet&status=active` |
| `GET /admin/sessions/:id` | Tek session detayı (token/maliyet bilgileri) |
| `GET /admin/logs` | Log dosyalarını okur, key/tarih ile filtreleme. `?after=<timestamp>` parametresi ile incremental fetch |
| `GET /admin/keys` | API key listesi + her birinin toplam kullanım istatistikleri |
| `GET /admin/costs` | Zaman serisi maliyet verisi (günlük/haftalık/aylık gruplama) |

### Admin Key Belirleme

`Config` struct'ına `admin_key_id: String` alanı eklenir. `parse_keys` fonksiyonu comma-separated string'i parse ederken, ilk key'in ID'sini bu alana yazar (HashMap'e insert etmeden önce, sıra garanti altında). Otomatik üretilen key durumunda o key admin olur. Bu sayede `HashMap`'in sırasız yapısından bağımsız olarak admin key her zaman belirlidir.

### Auth: Session-Based

1. `/dashboard` açılınca login formu gösterilir. Admin key JSON body ile gönderilir: `POST /admin/login` → `{ "key": "sk-..." }`
2. Rust tarafı key'i doğrular, kısa ömürlü session token üretir (1 saat TTL)
3. Token `httpOnly` + `SameSite=Strict` cookie olarak set edilir. `Secure` flag sadece HTTPS'te aktif (localhost'ta devre dışı, development uyumluluğu için)
4. Svelte tarafı token'ı hiç görmez — browser cookie'yi otomatik gönderir
5. Süre dolunca tekrar login gerekir

**Token storage:** `DashMap<String, AdminSession>` — token string → expiry timestamp. Sunucu restart'ında token'lar kaybolur (tekrar login gerekir, kabul edilebilir). Expired token'lar her login isteğinde lazy cleanup ile temizlenir.

**CSRF koruması:** `SameSite=Strict` cookie policy ile state-changing istekler korunur. Tüm admin endpointleri aynı origin'den gelmelidir.

Bu yaklaşım XSS saldırılarına karşı dayanıklıdır çünkü JavaScript token'a erişemez.

### Veri Kaynakları

- Overview, sessions, keys → mevcut `DashMap` in-memory state'ten
- Logs, costs → disk üzerindeki JSONL log dosyalarından parse
- Toplam istek sayısı → yeni `AtomicU64` global counter (mevcut handler'larda increment edilir). Restart'ta sıfırlanır; geçmiş veriler log dosyalarından hesaplanır.

### Maliyet Aggregation

`GET /admin/costs` endpoint'i JSONL log dosyalarını parse eder ve belirtilen gruplama (günlük/haftalık/aylık) ile aggregate eder. İlk versiyonda cache yok — log dosyaları küçükken (aylık rotation) yeterli performans. İleride gerekirse in-memory cache eklenebilir.

## Dashboard Sayfaları

### Overview (Ana Sayfa)
- Üstte 4 kart: uptime, toplam istek, aktif session, toplam maliyet (USD)
- Günlük istek/maliyet çizgi grafiği (son 30 gün)
- Model bazlı kullanım dağılımı (pasta grafik)

### Sessions
- Tablo: session ID, oluşturulma tarihi, model, mesaj sayısı, maliyet, durum (aktif/kapalı)
- Sıralama ve filtreleme (model, tarih aralığı)
- Satıra tıklayınca detay: token kullanımı, maliyet breakdown (mesaj geçmişi içeriği gösterilmez — sadece metadata)

### Logs
- Key ve tarih bazlı filtreleme
- Log seviyesi renk kodlaması (INFO yeşil, WARN sarı, ERROR kırmızı)
- Polling ile güncelleme (5sn interval), `?after=<timestamp>` parametresi ile sadece yeni kayıtlar çekilir

### API Keys
- Key listesi: key ID, toplam istek, toplam maliyet
- Her key'in son 7 günlük kullanım sparkline'ı

### Cost Analytics
- Zaman bazlı maliyet grafiği (günlük/haftalık/aylık toggle)
- Model bazlı breakdown (stacked bar chart)
- Key bazlı maliyet karşılaştırması

### Grafik Kütüphanesi
Chart.js — hafif, Svelte ile kolay entegrasyon, binary boyutuna eklemez (frontend bundle'da kalır).

### UI Error States
- API unreachable → retry banner ile bildirim
- Session expired → otomatik login sayfasına yönlendirme
- Boş veri (yeni deployment) → empty state mesajları ("Henüz session yok" vb.)

## Teknik Detaylar

### Rust Tarafı
- `rust-embed` crate'i ile `dashboard/dist/` binary'ye gömülür
- `/dashboard` path'i altında `axum::Router` ile serve — `index.html` fallback (SPA routing)
- Feature gate: `#[cfg(feature = "dashboard")]` ile tüm dashboard kodu conditional
- Admin endpointler de aynı feature gate arkasında

### Svelte Tarafı
- SvelteKit değil, vanilla Svelte + Vite (daha hafif, SSR gereksiz)
- `svelte-spa-router` ile client-side routing (Svelte 5 uyumlu, hash-based routing)
- API client: basit `fetch` wrapper, cookie otomatik gönderilir (`credentials: 'same-origin'`)

### Build Pipeline (`build.rs`)
- `dashboard` feature aktifse: `dashboard/dist/` yoksa veya `dashboard/src/` değişmişse `npm install && npm run build` çalıştırır
- CI'da `node` kurulu olmalı (sadece bu feature için)
- Feature kapalıyse: hiçbir şey yapmaz
- Incremental: `dashboard/dist/` zaten mevcutsa ve kaynak değişmediyse rebuild atlanır

### Binary Boyut Etkisi
- Svelte build output genelde ~50-150 KB (gzip)
- `rust-embed` ile binary'ye ~200 KB ek (Chart.js dahil)
- Feature kapalıyken sıfır ek

## Dev Workflow

Geliştirme sırasında Svelte'in Vite dev server'ı `localhost:5173`'te çalışır, API isteklerini `localhost:3000`'e proxy eder. Rust tarafını her seferinde rebuild etmeye gerek kalmaz.
