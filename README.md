## Usage

for website: `ex.login.net.vn`

use `nmcli --overview` and look for the connection _name_ & _interface_

then edit the .env file for your connection, username, password

after that, `source .env` and run the script `./dorm-wifi-login.sh`

### note:

this script is for personal use on my own Linux laptop/server, so I can SSH into it and log in to my dorm network captive portal when the machine is running headless.

it is not intended to bypass payment, authentication, access control, or network policy. It only automates the same login flow I would normally complete manually in a browser using my own account credentials.
