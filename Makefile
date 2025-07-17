redis-start:
	docker run -d --name gas-station-redis -p 6379:6379 redis:7.2.5

redis-restart:
	docker stop gas-station-redis
	docker rm gas-station-redis -v
	docker run -d --name gas-station-redis -p 6379:6379 redis:7.2.5

redis-stop:
	docker stop gas-station-redis
	docker rm gas-station-redis -v

