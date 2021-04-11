Sorokdva-smarthome
==================

This idea of this project is to make an adapter of my own smarthome to Yandex.Alice dialog platform.

It consists of four main parts:

1. OAuth2 implementation. It is based on the great authlib [1]_ library, and own integration with aiohttp [2]_.
2. More or less full Yandex.Alice platform protocol implementation [3]_ [4]_.
3. aiohttp [2]_ based web server, providing necessary authorization, routing and access to devices.
4. Some set of devices implementation. They are mostly the ones, that I have but can be used as base and/or 
   inspiration for writing your own device adapters.

Running
-------

This server is intended to let you make your own secure smarthome platform.
No public access. No open access to MQTT or anything else.
No proprietary clouds from some faraway country.
You own your devices and give access to them only to a single specific service on your own choice.

To make your own smarthome platform you need:

1. Register your own skill on the dialog platform [5]_. Do not forget to make it private.
2. Next you  need your own server (we're making private platform after all). You will need some https certificate (e.g. Letsencrypt).
   Choose subpath on the server, that'll host our platform, e.g. ``/alice/``. Take a look at example config for nginx::

       upstream alice {
           server 127.0.0.1:8888;
       }

       server {
           listen 443 default_server ssl http2;
           listen [::]:443 default_server ssl http2;
           port_in_redirect off;

           ssl_certificate /etc/letsencrypt/live/mydomain.com/fullchain.pem;
           ssl_certificate_key /etc/letsencrypt/live/mydomain.com/privkey.pem;
           # ... other default ssl and server params

           location /alice/ {
               proxy_pass http://alice;
               proxy_set_header Host $http_host;
               proxy_set_header X-Real-IP $remote_addr;
               proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
               proxy_set_header X-Forwarded-Host $server_name;
               proxy_set_header X-Forwarded-Proto $scheme;
               proxy_redirect off;
               proxy_buffering off;
           }
       }

   Here ``127.0.0.1:8888`` is the address of the interface platform will listen, and ``mydomain.com`` is the domain we use.

3. Bootstrap the app::

    $ git clone https://github.com/torkve/sorokdva-smarthome
    $ cd sorokdva-smarthome
    $ python3 -m virtualenv ve
    $ source ve/bin/activate
    $ pip install -r requirements.txt
 
4. Run app in debug mode::

    $ export PYTHONPATH="."
    $ python dialogs/app.py -p 8888 --prefix /alice/ --proxy \
        --db db.sqlite \
        --mqtt-host 127.0.0.1 --mqtt-port 1883 \
        --mqtt-login user --mqtt-password password \
        --debug

5. Debug mode allows us to register users. We will use this feature only once to register a user for Yandex.Alice platform.

   Navigate to ``https://mydomain.com/alice/auth``

   Remember to replace ``mydomain.com`` with your domain, and ``/alice`` with your selected prefix.

   Register some user, e.g. with name ``alice_user``.

   Login as this user on the same page. You will see the "Create client" button.

   According with the Alice platform, some fields must be fixed:

   * client URI: ``https://dialogs.yandex.ru``
   * redirect URI: ``https://social.yandex.net/broker/redirect``
   * allowed grant types: ``authorization_code`` and ``refresh_token`` (split by newline)
   * allowed response types: ``code``

   You can define any space-separated scopes if you wish to separate some privileges in the future.

   Client name may be anything.

   Write down ``client_id`` and ``client_secret`` of the app created.

6. Edit file ``app.toml`` to configure devices you have. Take ``app.toml.example`` for inspiration.

7. Restart app without debug mode::

    $ python dialogs/app.py -p 8888 --prefix /alice/ --proxy \
        --db db.sqlite \
        --mqtt-host 127.0.0.1 --mqtt-port 1883 \
        --mqtt-login user --mqtt-password password

8. Now you may edit you smarthome skill [5]_ and add backend endpoint url to the skill: ``https://mydomain.com/alice``.

9. Open account linking settings and set ``client identifier`` and ``client secret`` with the values you wrote in step 5.

   Set authorization url to ``https://mydomain.com/alice/auth``

   Set token endpoint to ``https://mydomain.com/alice/oauth/token``

   Set refresh token endpoint to ``https://mydomain.com/alice/oauth/token``

   Set scopes to the scopes you selected while creating the app.

10. Add your skill in the Yandex app. It will require you to authorize with the login you created in the step 5, then will request oauth access.

11. After that you should see your devices in the Yandex app and Yandex.Alice will respond to your requests.

I want easier way, why so manysteps and no Docker?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Feel free to send PRs.

Footnotes
=========

.. [1] https://github.com/lepture/authlib
.. [2] https://github.com/aio-libs/aiohttp
.. [3] https://yandex.ru/dev/dialogs/alice/doc/smart-home/about.html
.. [4] Right now there's now support for notifications API, but WIP.
.. [5] https://dialogs.yandex.ru/developer
