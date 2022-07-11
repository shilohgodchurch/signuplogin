use crate::cookies::{Authenticated, Unauthenticated};
use crate::user::rand_string;
use crate::{cookies, prelude::*, CsrfToken};
use rocket::http::{self, Cookie, CookieJar};
use rocket::request::FromRequest;
use rocket::request::Outcome;
use rocket::Request;
use rocket::State;
use serde_json::{json, to_string};
use std::time::Duration;

/// The [`Auth`] guard allows to log in, log out, sign up, modify, and delete the currently (un)authenticated user.
/// For more information see [`Auth`].
///  A working example:
/// ```rust,no_run
///
/// use rocket::{*, form::Form};
/// use rocket_auth::{Users, Error, Auth, Signup, Login};
///
/// #[post("/signup", data="<form>")]
/// async fn signup(form: Form<Signup>, auth: Auth<'_>) {
///     auth.signup(&form).await;
///     auth.login(&form.into());
/// }
///
/// #[post("/login", data="<form>")]
/// fn login(form: Form<Login>, auth: Auth) {
///     auth.login(&form);
/// }
///
/// #[post("/logout")]
/// fn logout(auth: Auth) {
///     auth.logout();
/// }
/// #[tokio::main]
/// async fn main() -> Result<(), Error>{
///     let users = Users::open_sqlite("mydb.db").await?;
///
///     rocket::build()
///         .mount("/", routes![signup, login, logout])
///         .manage(users)
///         .launch()
///         .await;
///     Ok(())
/// }
/// ```
#[allow(missing_docs)]
pub struct Auth<'a> {
    /// `Auth` includes in its fields a [`Users`] instance. Therefore, it is not necessary to retrieve `Users` when using this guard.
    pub users: &'a State<Users>,
    pub cookies: &'a CookieJar<'a>,
    pub(crate) session: Option<Session>,
    pub(crate) method: http::Method,
}

#[async_trait]
impl<'r> FromRequest<'r> for Auth<'r> {
    type Error = Error;
    async fn from_request(req: &'r Request<'_>) -> Outcome<Auth<'r>, Error> {
        let session = req.guard().await.succeeded();
        let users: &State<Users> = try_outcome!(
            req.guard().await,
            err: InternalServerError::UnmanagedStateError
        );
        Outcome::Success(Auth {
            users,
            session,
            cookies: req.cookies(),
            method: req.method(),
        })
    }
}

impl<'a> Auth<'a> {
    /// Returns the csrf token for this user. If the user
    /// does not have a csrf token, one will be created
    /// for them. It is ok to call this method twice. The
    /// csrf token will last for as long as the user
    /// session lasts.
    pub async fn csrf_token(&self) -> CsrfToken {
        match &self.session {
            Some(session) => session.csrf_token(),
            None => {
                // this is necessary so if users call `crsf_token()` twice
                // the previous csrf token doesn't get invalidated by resetting the
                // rocket_auth cookie
                if let Some(session) = Session::from_cookies(self.cookies) {
                    return session.csrf_token();
                }
                self.create_session().await.csrf_token()
            }
        }
    }

    async fn create_session(&self) -> Session {
        let session = Session::Unauthenticated(Unauthenticated {
            session_id: rand_string(32),
        });
        let cookie_value = to_string(&session).unwrap();
        self.cookies
            .add_private(Cookie::new("rocket_auth", cookie_value));
        session
    }

    /// Logs in the user through a parsed form or json.
    /// The session is set to expire in one year by default.
    /// For a custom expiration date use [`Auth::login_for`].
    /// ```rust
    /// # use rocket::{get, post, form::Form};
    /// # use rocket_auth::{Auth, Login};
    /// #[post("/login", data="<form>")]
    /// fn login(form: Form<Login>, auth: Auth) {
    ///     auth.login(&form);
    /// }
    /// ```
    #[throws(Error)]
    pub async fn login(&self, form: &Login) {
        self.assert_write_method()?;
        let key = self.users.login(form).await?;
        let user = self.users.get_by_email(&form.email.to_lowercase()).await?;
        let session = Session::Authenticated(cookies::Authenticated {
            id: user.id,
            email: user.email,
            session_id: key,
            timestamp: now(),
        });
        let to_str = to_string(&session).unwrap();
        self.cookies.add_private(Cookie::new("rocket_auth", to_str));
    }

    /// Logs a user in for the specified period of time.
    /// ```rust
    /// # use rocket::{post, form::Form};
    /// # use rocket_auth::{Login, Auth};
    /// # use std::time::Duration;
    /// #[post("/login", data="<form>")]
    /// fn login(form: Form<Login>, auth: Auth) {
    ///     let one_hour = Duration::from_secs(60 * 60);
    ///     auth.login_for(&form, one_hour);
    /// }
    /// ```
    #[throws(Error)]
    pub async fn login_for(&self, form: &Login, time: Duration) {
        self.assert_write_method()?;
        let key = self.users.login_for(form, time).await?;
        let user = self.users.get_by_email(&form.email.to_lowercase()).await?;

        let session = Session::Authenticated(Authenticated {
            id: user.id,
            email: user.email,
            session_id: key,
            timestamp: now(),
        });
        let to_str = format!("{}", json!(session));
        let cookie = Cookie::new("rocket_auth", to_str);
        self.cookies.add_private(cookie);
    }

    /// Creates a new user from a form or a json. The user will not be authenticated by default.
    /// In order to authenticate the user, cast the signup form to a login form or use `signup_for`.
    /// ```rust
    /// # use rocket::{post, form::Form};
    /// # use rocket_auth::{Auth, Signup, Error};
    /// # use std::time::Duration;
    /// #[post("/signup", data="<form>")]
    /// async fn signup(form: Form<Signup>, auth: Auth<'_>) -> Result<&'static str, Error>{
    ///     auth.signup(&form).await?;
    ///     auth.login(&form.into()).await?;
    ///     Ok("Logged in")
    /// }
    /// ```
    #[throws(Error)]
    pub async fn signup(&self, form: &Signup) {
        self.assert_write_method()?;
        self.users.signup(form).await?;
    }

    /// Creates a new user from a form or a json.
    /// The session will last the specified period of time.
    /// ```rust
    /// # use rocket::{post, form::Form};
    /// # use rocket_auth::{Auth, Signup};
    /// # use std::time::Duration;
    /// #[post("/signup", data="<form>")]
    /// fn signup_for(form: Form<Signup>, auth: Auth) {
    ///     let one_hour = Duration::from_secs(60 * 60);
    ///     auth.signup_for(&form, one_hour);
    /// }
    /// ```
    #[throws(Error)]
    pub async fn signup_for(&self, form: &Signup, time: Duration) {
        self.assert_write_method()?;
        self.users.signup(form).await?;
        self.login_for(&form.clone().into(), time).await?;
    }

    ///
    ///
    /// It allows to know if the current client is authenticated or not.
    /// ```rust
    /// # use rocket::{get};
    /// # use rocket_auth::{Auth};
    /// #[get("/am-I-authenticated")]
    /// async fn is_auth(auth: Auth<'_>) -> &'static str {
    ///     if auth.is_auth().await {
    ///         "Yes you are."
    ///     } else {
    ///         "nope."
    ///     }
    /// }
    /// ```
    pub async fn is_auth(&self) -> bool {
        if let Some(session) = &self.session {
            self.users.is_auth(session).await
        } else {
            false
        }
    }

    /// It retrieves the current logged user.  
    /// ```
    /// # use rocket::get;
    /// # use rocket_auth::Auth;
    /// #[get("/display-me")]
    /// async fn display_me(auth: Auth<'_>) -> String {
    ///     format!("{:?}", auth.get_user().await)
    /// }
    /// ```
    pub async fn get_user(&self) -> Option<User> {
        if !self.is_auth().await {
            return None;
        }
        let id = self.session.as_ref()?.id().ok()?;
        if let Ok(user) = self.users.get_by_id(id).await {
            Some(user)
        } else {
            None
        }
    }

    /// Logs the currently authenticated user out.
    /// ```rust
    /// # use rocket::post;
    /// # use rocket_auth::Auth;
    /// #[post("/logout")]
    /// fn logout(auth: Auth)  {
    ///     auth.logout();
    /// }
    /// ```
    #[throws(Error)]
    pub async fn logout(&self) {
        self.assert_write_method()?;
        let session = self.get_session()?;
        self.users.logout(session).await?;
        self.cookies.remove_private(Cookie::named("rocket_auth"));
    }
    /// Deletes the account of the currently authenticated user.
    /// ```rust
    /// # use rocket::post;
    /// # use rocket_auth::Auth;
    /// #[post("/delete-my-account")]
    /// fn delete(auth: Auth)  {
    ///     auth.delete();
    /// }
    /// ```
    #[throws(Error)]
    pub async fn delete(&self) {
        self.assert_write_method()?;
        if self.is_auth().await {
            let session = self.get_session()?;
            self.users.delete(session.id()?).await?;
            self.cookies.remove_private(Cookie::named("rocket_auth"));
        } else {
            throw!(Error::Unauthorized)
        }
    }

    /// Changes the password of the currently authenticated user
    /// ```
    /// # use rocket_auth::Auth;
    /// # use rocket::post;
    /// # #[post("/change")]
    /// # fn example(auth: Auth<'_>) {
    ///     auth.change_password("new password");
    /// # }
    /// ```
    #[throws(Error)]
    pub async fn change_password(&self, password: &str) {
        self.assert_write_method()?;
        if self.is_auth().await {
            let session = self.get_session()?;
            let mut user = self.users.get_by_id(session.id()?).await?;
            user.set_password(password)?;
            self.users.modify(&user).await?;
        } else {
            throw!(Error::Unauthorized)
        }
    }

    /// Changes the email of the currently authenticated user
    /// ```
    /// # use rocket::post;
    /// # use rocket_auth::{Auth, Result};
    /// #[post("/user/change-email", data="<new_email>")]
    /// async fn change_email(new_email: String, auth: Auth<'_>) -> Result {
    ///     auth.change_email(new_email).await?;
    ///     Ok(())
    /// }
    /// ```
    #[throws(Error)]
    pub async fn change_email(&self, email: String) {
        self.assert_write_method()?;
        if self.is_auth().await {
            if !validator::validate_email(&email) {
                throw!(ValidationError::InvalidEmailAddress)
            }
            let session = self.get_session()?;
            let mut user = self.users.get_by_id(session.id()?).await?;
            user.email = email.to_lowercase();
            self.users.modify(&user).await?;
        } else {
            throw!(Error::Unauthorized)
        }
    }

    /// Compares the password of the currently authenticated user with another password.
    /// Useful for checking password before resetting email/password.
    /// To avoid bruteforcing this function should not be directly accessible from a route.
    /// Additionally, it is good to implement rate limiting on routes using this function.
    #[throws(Error)]
    pub async fn compare_password(&self, password: &str) -> bool {
        if self.is_auth().await {
            let session = self.get_session()?;
            let user: User = self.users.get_by_id(session.id()?).await?;
            user.compare_password(password)?
        } else {
            throw!(Error::Unauthorized)
        }
    }

    /// This method is useful when the function returns a Result type.
    /// It is intended to be used primarily
    /// with the `?` operator.
    /// ```
    /// # fn func(auth: rocket_auth::Auth) -> Result<(), rocket_auth::Error> {
    /// auth.get_session()?;
    /// # Ok(())
    /// # }
    /// ```
    #[throws(Error)]
    pub(crate) fn get_session(&self) -> &Session {
        let session = self.session.as_ref().ok_or(Error::Unauthorized)?;
        session
    }

    /// Determines if the http method for this
    /// handler is safe (read only) or unsafe (read or write).
    /// This is used to error out stateful actions performed
    /// through read only endpoints.
    /// ```rust
    /// self.assert_unsafe_method()?;
    /// ```
    #[throws(Error)]
    fn assert_write_method(&self) {
        use http::Method::*;
        if let Get | Trace | Options | Head = self.method {
            throw!(Error::HttpMethod(self.method));
        }
    }
}
