# Weather executable

Weather demo app for Xila WASM runtime.

## Data sources

- Geocoding: `https://geocoding-api.open-meteo.com/v1/search`
- Forecast: `https://api.open-meteo.com/v1/forecast`

## Runtime path

Uses internal HTTPS device mounted at `/devices/network/https_client`.

## UX behavior

- Enter city name
- Press Refresh
- Auto-picks first geocoding result
- Displays Current, Hourly, Daily, and Meta tabs
