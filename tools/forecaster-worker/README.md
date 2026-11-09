# WRFCoin forecaster worker (v0)

Run a small program on your own computer that forecasts the weather at WRFCoin stations and
puts each forecast on the WRFCoin test network (chain 888). No GPU. No account. No email.

What it does, by itself:

1. Every hour it reads the public readings of the stations on [wrfcoin.com](https://wrfcoin.com).
2. It downloads a tiny piece of the free NOAA weather model (GFS) for each station.
3. It learns how wrong the model usually is **at that station** and corrects for it.
4. Once a day it sends its forecasts for 1, 6, 12 and 24 hours ahead, signed by its own wallet.

The network is built to pay for forecasts that are **better** than the public model, not for
computer power. The correction in step 3 is the part that can earn. It gets better after a few
days of running, because the worker needs past readings to learn from.

> This is a test network. Test coins have no money value. Pay for forecast skill is not switched
> on yet; today you get a score, not coins. See "What works today" below.

## Run it

1. Install Docker: <https://docs.docker.com/get-docker/>
2. Get this folder and build the worker (about one minute):

   ```sh
   git clone https://github.com/wrfcoin/core4.git
   cd core4/tools/forecaster-worker
   mkdir -p worker-data
   printf 'WORKER_UID=%s\nWORKER_GID=%s\n' "$(id -u)" "$(id -g)" > .env
   docker compose build
   ```

   The `printf` line is for Linux and macOS: it makes the worker run as you, so the wallet file
   belongs to you. On Windows (PowerShell) skip that line and use `mkdir worker-data`.

3. Try it first without sending anything:

   ```sh
   docker compose run --rm worker once --dry-run
   ```

   It prints your wallet address and the forecasts it **would** send. A dry run does not log in
   and signs nothing.

4. Start it for real. It keeps running and restarts with your computer:

   ```sh
   docker compose up -d
   docker compose logs -f worker
   ```

To stop it: `docker compose down`. Your wallet and history stay in the `worker-data` folder.

## Your wallet, your address, your score

- The first run makes a new wallet. The worker prints its **address** (it starts with `0x`).
  Print it again any time: `docker compose run --rm worker address`
- The 12 secret words of the wallet are in the file `worker-data/wallet-phrase.txt`. The worker
  never shows them. On Linux and macOS the file is readable only by your user. On Windows the
  file has the same access as the folder it is in, so keep the folder inside your own user
  folder. **Keep that file private and keep a copy.** Whoever has the words has the wallet.
- See your forecasts and scores in the terminal: `docker compose run --rm worker status`
- See them on the site: <https://wrfcoin.com/forecast/new> lists the forecasts of the signed-in
  wallet. Sign in there with a 12-word wallet, using the words from your file.
- The public forecaster table is at <https://wrfcoin.com/forecasters>.

A score appears after the forecast's target time has passed and the station has reported.

## Settings

Change them in `compose.yml` under `environment:`. The defaults are fine to start.

| Setting | Default | Meaning |
|---|---|---|
| `WORKER_STATIONS` | `auto` | `auto` = every station the site takes forecasts for. Or a list like `st_0123456789ab,st_ba9876543210`. |
| `WORKER_VARIABLE` | `temperature_c` | `temperature_c` or `pressure_hpa`. The site counts one forecast per station, lead and day. |
| `WORKER_LEADS` | `1,6,12,24` | Hours ahead to forecast. |
| `WORKER_CYCLE_SECONDS` | `3600` | How often the worker wakes up. Not less than 900. |
| `WRFCOIN_API` | `https://wrfcoin.com/api` | The site to talk to. |

## What works today

- The worker can only forecast stations the site has opened for forecasts. If the log says
  "No station to forecast this cycle", none is open right now. Leave it running.
- The site keeps the **first** forecast per wallet, station, lead and day. The worker sends one and stops.
- A station that has gone silent gets no forecast, because nothing could score it.
- New workers start with the plain model value. The correction grows as history builds up
  (it is cautious until it has about a week of samples).

## How it is built

- Plain Python, no extra packages. Tests: `python3 -m unittest discover -s tests -v`
- Network use is small: about 4 requests of under 1 kB to NOAA per station per hour, and one read
  of each station's readings per hour from wrfcoin.com.
- It stores station readings and its own forecasts in `worker-data/state.json`. It stores no
  station position.
- The forecast is signed on your computer. The site relays it to the chain and reveals it; the
  worker checks every draft the site returns before signing it.

## Data credit

Weather model data: NOAA/NCEP Global Forecast System (GFS), from NOMADS
(<https://nomads.ncep.noaa.gov>). NOAA data is in the public domain. This is not an official NOAA
product, and NOAA does not endorse this software.
The word list in `forecaster_worker/bip39_english.txt` is the BIP-39 English list (MIT licence,
<https://github.com/bitcoin/bips/blob/master/bip-0039/english.txt>).
