# User Authentication System

This server now requires user authentication before allowing communication. Users must register or login before they can send messages.

## Authentication Flow

1. Client connects to the server
2. Server sends an authentication prompt
3. Client must send either a `login` or `register` command
4. After successful authentication, client can communicate normally

## Authentication Commands

### Register New User
```json
{
  "command": "register",
  "data": "{\"passphrase\": \"your_unique_passphrase\", \"password\": \"your_password\"}"
}
```

### Login Existing User
```json
{
  "command": "login", 
  "data": "{\"passphrase\": \"your_unique_passphrase\", \"password\": \"your_password\"}"
}
```

## Authentication Response
The server will respond with:
```json
{
  "command": "auth_response",
  "data": "{\"success\": true/false, \"message\": \"status message\", \"user_id\": 123}"
}
```

## Example Client Session

1. Connect to server
2. Server sends: `{"command": "auth_required", "data": "Please authenticate..."}`
3. Client sends registration: `{"command": "register", "data": "{\"passphrase\": \"alice\", \"password\": \"secret123\"}"}`
4. Server responds: `{"command": "auth_response", "data": "{\"success\": true, \"message\": \"Registration successful\", \"user_id\": 1}"}`
5. Now client can send normal messages and they will be echoed back

## Security Features

- Passwords are hashed using SHA-256 before storage
- Each passphrase must be unique
- Users cannot communicate without authentication
- User sessions are maintained per connection
