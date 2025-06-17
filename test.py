# /// script
# dependencies = [
#   'requests',
# ]
# ///

import requests

url = "https://open-api.bingx.com/openApi/swap/v3/quote/klines"
params = {
    "symbol": "BTC-USDT",
    "interval": "1m",  # or 5m, 1h, etc.
    "limit": 360,
}
response = requests.get(url, params=params)
data = response.json()
print(data)
